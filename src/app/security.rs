use std::{
    collections::HashMap,
    time::{Duration, Instant, SystemTime},
};

use anyhow::{Context, Result, ensure};
use backend::{
    DataProvider, Entry,
    protection::{self, SecretString},
};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use super::App;

type PrivateEntry = scopeguard::ScopeGuard<Entry, fn(Entry)>;

fn private_entry(entry: Entry) -> PrivateEntry {
    fn erase(mut entry: Entry) {
        entry.content.zeroize();
        entry.tags.zeroize();
        entry.priority.zeroize();
        entry.mood.zeroize();
        entry.journal_prompt.zeroize();
    }
    scopeguard::guard(entry, erase as fn(Entry))
}

pub(super) struct UnlockedSession {
    pub entry: Entry,
    password: SecretString,
    native_metadata: Option<String>,
}

impl Drop for UnlockedSession {
    fn drop(&mut self) {
        self.entry.content.zeroize();
        self.entry.tags.zeroize();
        self.entry.priority.zeroize();
        self.entry.mood.zeroize();
        self.entry.journal_prompt.zeroize();
        self.native_metadata.zeroize();
    }
}

pub(super) struct SealedDraft {
    entry: Entry,
    base: Entry,
}

#[derive(Serialize, Deserialize, PartialEq, Eq)]
pub(crate) struct NoteDraft {
    pub content: String,
    pub entry_popup: Option<String>,
}

impl std::fmt::Debug for NoteDraft {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("NoteDraft { [REDACTED] }")
    }
}

impl std::ops::Deref for NoteDraft {
    type Target = str;
    fn deref(&self) -> &str {
        &self.content
    }
}

impl Drop for NoteDraft {
    fn drop(&mut self) {
        self.content.zeroize();
        self.entry_popup.zeroize();
    }
}

pub(super) struct SecurityState {
    pub sessions: HashMap<u32, UnlockedSession>,
    drafts: HashMap<u32, SealedDraft>,
    last_activity: Instant,
    last_wall_activity: SystemTime,
}

impl Default for SecurityState {
    fn default() -> Self {
        Self {
            sessions: HashMap::new(),
            drafts: HashMap::new(),
            last_activity: Instant::now(),
            last_wall_activity: SystemTime::now(),
        }
    }
}

impl<D: DataProvider> App<D> {
    pub fn journal_is_protected(&self) -> bool {
        self.data_provide.is_journal_protected()
    }

    pub fn locked_note_ids(&self) -> Vec<u32> {
        self.entries
            .iter()
            .filter(|entry| self.is_locked(entry.id))
            .map(|entry| entry.id)
            .collect()
    }

    pub async fn enable_journal_protection(&mut self, password: SecretString) -> Result<()> {
        ensure!(
            !self.journal_is_protected(),
            "This journal is already protected"
        );
        ensure!(
            !self.has_sealed_drafts(),
            "Unlock and save or discard all encrypted drafts first"
        );
        ensure!(
            self.entries.iter().all(|entry| entry.protection.is_none()
                || self.security.sessions.contains_key(&entry.id)),
            "Unlock every individually protected entry using Entry security before protecting the journal. Their separate passwords will be replaced by the journal password."
        );

        let result = async {
            let mut records = self.data_provide.journal_records().await?;
            for record in &mut records {
                if record.entry.protection.is_some() {
                    let session = self.security.sessions.get(&record.entry.id).context(
                        "Unlock every individually protected entry before protecting the journal",
                    )?;
                    ensure!(
                        session.entry.protection == record.entry.protection,
                        "An entry changed outside this session; reopen it before protecting the journal"
                    );
                    record.entry = session.entry.clone();
                    record.entry.protection = None;
                    record.native_metadata = session.native_metadata.clone();
                }
            }
            self.data_provide.enable_journal_protection(password, Some(records)).await
        }.await;
        if self.journal_is_protected() {
            self.invalidate_converted_entries();
        }
        self.check_journal_conversion_result(result)?;
        self.reload_after_journal_conversion().await
    }

    pub async fn change_journal_password(
        &mut self,
        current: SecretString,
        new: SecretString,
    ) -> Result<()> {
        self.data_provide
            .change_journal_password(current, new)
            .await
    }

    pub async fn disable_journal_protection(&mut self, password: SecretString) -> Result<()> {
        let result = self.data_provide.disable_journal_protection(password).await;
        if !self.journal_is_protected() {
            self.invalidate_converted_entries();
        }
        self.check_journal_conversion_result(result)?;
        self.reload_after_journal_conversion().await
    }

    fn check_journal_conversion_result(&mut self, result: Result<()>) -> Result<()> {
        if let Err(error) = result {
            if self.data_provide.journal_reload_required() {
                self.invalidate_converted_entries();
                return Err(error.context("Close and reopen this journal before editing."));
            }
            return Err(error);
        }
        Ok(())
    }

    async fn reload_after_journal_conversion(&mut self) -> Result<()> {
        self.invalidate_converted_entries();
        self.load_entries().await.context("Journal protection changed, but entries could not be reloaded; close and reopen this journal")?;
        self.current_entry_id = self.entries.first().map(|entry| entry.id);
        Ok(())
    }

    fn invalidate_converted_entries(&mut self) {
        self.history = super::history::HistoryManager::new(self.settings.history_limit);
        self.security.sessions.clear();
        self.selected_entries.clear();
        self.filter = None;
        self.filtered_out_entries.clear();
        self.current_entry_id = None;
        for entry in &mut self.entries {
            entry.title.zeroize();
            entry.content.zeroize();
            entry.tags.zeroize();
            entry.priority.zeroize();
            entry.mood.zeroize();
            entry.journal_prompt.zeroize();
        }
        self.entries.clear();
        self.update_colored_tags();
        self.redraw_after_restore = true;
    }

    pub fn entry_mood(&self, entry: &Entry) -> Option<backend::Mood> {
        if entry.protection.is_some() {
            self.security
                .sessions
                .get(&entry.id)
                .and_then(|session| session.entry.mood)
        } else {
            entry.mood
        }
    }

    pub fn is_protected(&self, id: u32) -> bool {
        self.entries
            .iter()
            .any(|e| e.id == id && e.protection.is_some())
    }

    pub fn is_locked(&self, id: u32) -> bool {
        self.is_protected(id) && !self.security.sessions.contains_key(&id)
    }

    pub fn current_is_protected(&self) -> bool {
        self.current_entry_id
            .is_some_and(|id| self.is_protected(id))
    }

    pub fn current_is_locked(&self) -> bool {
        self.current_entry_id.is_some_and(|id| self.is_locked(id))
    }

    pub fn has_sealed_drafts(&self) -> bool {
        !self.security.drafts.is_empty()
    }

    pub fn has_sealed_draft(&self, id: u32) -> bool {
        self.security.drafts.contains_key(&id)
    }

    pub fn discard_sealed_draft(&mut self, id: u32) {
        self.security.drafts.remove(&id);
    }

    pub fn record_activity(&mut self) {
        self.security.last_activity = Instant::now();
        self.security.last_wall_activity = SystemTime::now();
    }

    pub fn security_expired(&self) -> bool {
        let seconds = self.settings.security.auto_lock_seconds;
        seconds > 0
            && !self.security.sessions.is_empty()
            && (self.security.last_activity.elapsed() >= Duration::from_secs(seconds)
                || self
                    .security
                    .last_wall_activity
                    .elapsed()
                    .is_ok_and(|elapsed| elapsed >= Duration::from_secs(seconds)))
    }

    pub fn lock_note(&mut self, id: u32) {
        if !self.is_protected(id) && !self.security.sessions.contains_key(&id) {
            return;
        }
        self.security.sessions.remove(&id);
        self.history.forget_entry(id);
        self.retain_restore_metadata();
    }

    pub async fn protect_note(
        &mut self,
        id: u32,
        password: SecretString,
        content: String,
    ) -> Result<()> {
        ensure!(
            !self.journal_is_protected(),
            "This journal already protects every entry; use journal security in Config"
        );
        ensure!(!self.is_protected(id), "This note is already protected");
        let mut candidate = private_entry(self.get_entry(id).context("No note selected")?.clone());
        candidate.content.zeroize();
        candidate.content = content;
        let metadata = self.data_provide.protection_metadata(id).await?;
        let encrypted = tokio::task::spawn_blocking(move || {
            let envelope = protection::protect(&candidate, &password, metadata)?;
            Ok::<_, anyhow::Error>(
                scopeguard::ScopeGuard::into_inner(candidate).with_protection(envelope),
            )
        })
        .await
        .context("Encryption task failed")??;
        let persisted = self.data_provide.update_entry(encrypted).await?;
        self.replace_protected_record(persisted);
        self.lock_note(id);
        Ok(())
    }

    pub async fn unlock_note(
        &mut self,
        id: u32,
        password: SecretString,
    ) -> Result<Option<NoteDraft>> {
        let stored = self
            .entries
            .iter()
            .find(|e| e.id == id)
            .context("No note selected")?
            .clone();
        ensure!(stored.protection.is_some(), "This note is not protected");
        if let Some(draft) = self.security.drafts.get(&id) {
            ensure!(
                draft.base == stored,
                "The stored note changed. Its unsaved encrypted draft has been retained; restore the original note before reopening the draft."
            );
        }
        let draft = self.security.drafts.get(&id).map(|d| d.entry.clone());
        let (session, draft_content) = tokio::task::spawn_blocking(move || {
            let mut private = protection::unlock(&stored, &password)?;
            let mut entry = private_entry(stored);
            entry.content = std::mem::take(&mut private.content);
            entry.tags = std::mem::take(&mut private.tags);
            entry.priority = private.priority;
            entry.mood = private.mood;
            entry.journal_prompt = private.journal_prompt.take();
            let draft_content = if let Some(draft) = draft {
                let private_draft = protection::unlock(&draft, &password)?;
                Some(
                    serde_json::from_str::<NoteDraft>(&private_draft.content)
                        .context("Could not restore the encrypted draft")?,
                )
            } else {
                None
            };
            Ok::<_, anyhow::Error>((
                UnlockedSession {
                    entry: scopeguard::ScopeGuard::into_inner(entry),
                    password,
                    native_metadata: private.native_metadata.take(),
                },
                draft_content,
            ))
        })
        .await
        .context("Decryption task failed")??;
        self.security.sessions.insert(id, session);
        self.security.drafts.remove(&id);
        self.record_activity();
        Ok(draft_content)
    }

    pub(super) async fn save_protected_entry(&mut self, candidate: Entry) -> Result<()> {
        let mut candidate = private_entry(candidate);
        let id = candidate.id;
        let session = self
            .security
            .sessions
            .get(&id)
            .context("Unlock this note before editing it")?;
        candidate.protection = session.entry.protection.clone();
        let password = session.password.clone();
        let metadata = session.native_metadata.clone();
        let (candidate, encrypted) = tokio::task::spawn_blocking(move || {
            let envelope = protection::reseal(&candidate, &password, metadata)?;
            let encrypted = candidate.clone().with_protection(envelope);
            Ok::<_, anyhow::Error>((candidate, encrypted))
        })
        .await
        .context("Encryption task failed")??;
        let persisted = self.data_provide.update_entry(encrypted).await?;
        let session = self
            .security
            .sessions
            .get_mut(&id)
            .expect("session remains open during save");
        session.entry.content.zeroize();
        session.entry.tags.zeroize();
        session.entry = scopeguard::ScopeGuard::into_inner(candidate);
        session.entry.protection = persisted.protection.clone();
        self.replace_protected_record(persisted);
        self.security.drafts.remove(&id);
        Ok(())
    }

    pub async fn change_note_password(&mut self, id: u32, password: SecretString) -> Result<()> {
        let session = self
            .security
            .sessions
            .get(&id)
            .context("Unlock this note first")?;
        let candidate = private_entry(session.entry.clone());
        let metadata = session.native_metadata.clone();
        let (password, encrypted) = tokio::task::spawn_blocking(move || {
            let envelope = protection::reseal(&candidate, &password, metadata)?;
            Ok::<_, anyhow::Error>((
                password,
                scopeguard::ScopeGuard::into_inner(candidate).with_protection(envelope),
            ))
        })
        .await
        .context("Encryption task failed")??;
        let persisted = self.data_provide.update_entry(encrypted).await?;
        let session = self
            .security
            .sessions
            .get_mut(&id)
            .expect("session remains open during password change");
        session.password = password;
        session.entry.protection = persisted.protection.clone();
        self.replace_protected_record(persisted);
        Ok(())
    }

    pub async fn remove_note_protection(&mut self, id: u32) -> Result<()> {
        let session = self
            .security
            .sessions
            .get(&id)
            .context("Unlock this note first")?;
        let mut candidate = session.entry.clone();
        candidate.protection = None;
        let metadata = session.native_metadata.clone();
        let persisted = self
            .data_provide
            .update_entry_with_metadata(candidate, metadata.as_deref())
            .await?;
        self.replace_protected_record(persisted);
        self.lock_note(id);
        self.security.drafts.remove(&id);
        Ok(())
    }

    /// Encrypt unsaved editor text in memory before dropping its password and plaintext session.
    pub async fn seal_draft_and_lock(
        &mut self,
        id: u32,
        content: String,
        entry_popup: Option<String>,
    ) -> Result<()> {
        let session = self
            .security
            .sessions
            .get(&id)
            .context("No unlocked note")?;
        let mut candidate = private_entry(session.entry.clone());
        candidate.content.zeroize();
        candidate.content = serde_json::to_string(&NoteDraft {
            content,
            entry_popup,
        })?;
        let password = session.password.clone();
        let metadata = session.native_metadata.clone();
        let base = self
            .entries
            .iter()
            .find(|e| e.id == id)
            .context("No stored note")?
            .clone();
        let entry = tokio::task::spawn_blocking(move || {
            let envelope = protection::reseal(&candidate, &password, metadata)?;
            Ok::<_, anyhow::Error>(
                scopeguard::ScopeGuard::into_inner(candidate).with_protection(envelope),
            )
        })
        .await
        .context("Draft encryption task failed")??;
        self.security.drafts.insert(id, SealedDraft { entry, base });
        self.lock_note(id);
        Ok(())
    }

    pub fn lock_all_saved_notes(&mut self) {
        let ids: Vec<_> = self.security.sessions.keys().copied().collect();
        for id in ids {
            self.lock_note(id);
        }
    }

    fn replace_protected_record(&mut self, entry: Entry) {
        let id = entry.id;
        let old = self
            .entries
            .iter_mut()
            .find(|e| e.id == id)
            .expect("note remains loaded");
        old.content.zeroize();
        old.tags.zeroize();
        *old = entry;
        self.history.forget_entry(id);
        self.retain_restore_metadata();
        self.sort_entries();
        self.update_filter();
        self.update_filtered_out_entries();
        self.update_colored_tags();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "json")]
    #[tokio::test]
    async fn failed_conversion_discards_stale_app_view_before_any_further_save() {
        use backend::{
            EntryDraft,
            journal::{JournalBackend, JournalProvider},
        };
        let temporary = tempfile::tempdir().unwrap();
        let path = temporary.path().join("entries.json");
        let provider = JournalProvider::open(JournalBackend::Json(path.clone()))
            .await
            .unwrap();
        let mut app = App::new(provider, crate::settings::Settings::default());
        let stale = app
            .data_provide
            .add_entry(EntryDraft::new(
                chrono::Utc::now(),
                "Old title".into(),
                vec![],
                None,
            ))
            .await
            .unwrap();
        app.load_entries().await.unwrap();
        app.current_entry_id = Some(stale.id);
        let mut external: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let entries = if external.is_array() {
            external.as_array_mut().unwrap()
        } else {
            external["entries"].as_array_mut().unwrap()
        };
        entries[0]["title"] = "Externally changed title".into();
        entries[0]["custom_metadata"] = "Preserve this unsupported data".into();
        let changed = serde_json::to_vec(&external).unwrap();
        std::fs::write(&path, &changed).unwrap();
        let error = app
            .enable_journal_protection("test password".to_owned().into())
            .await
            .unwrap_err();
        assert!(error.to_string().contains("reopen"));
        assert!(!app.journal_is_protected());
        assert!(app.entries.is_empty());
        assert!(app.current_entry_id.is_none());
        assert!(app.data_provide.update_entry(stale).await.is_err());
        assert_eq!(std::fs::read(&path).unwrap(), changed);
        app.load_entries().await.unwrap();
        assert_eq!(app.entries[0].title, "Externally changed title");
    }

    #[cfg(feature = "json")]
    #[tokio::test]
    async fn confirmed_journal_export_does_not_refresh_write_baseline_behind_editor() {
        use backend::{
            EntryDraft,
            journal::{JournalBackend, JournalProvider},
        };
        let temporary = tempfile::tempdir().unwrap();
        let store = temporary.path().join("entries.json");
        let provider = JournalProvider::open(JournalBackend::Json(store.clone()))
            .await
            .unwrap();
        let mut settings = crate::settings::Settings::default();
        settings.backend_type = Some(crate::settings::BackendType::Json);
        settings.json_backend.file_path = Some(store.clone());
        let mut app = App::new(provider, settings);
        app.data_provide
            .add_entry(
                EntryDraft::new(chrono::Utc::now(), "Title".into(), vec![], None)
                    .with_content("Earlier body".into()),
            )
            .await
            .unwrap();
        app.load_entries().await.unwrap();
        app.enable_journal_protection("journal password".to_owned().into())
            .await
            .unwrap();
        let older_ciphertext = std::fs::read(&store).unwrap();
        app.update_current_entry_content("Displayed saved body".into())
            .await
            .unwrap();
        std::fs::write(&store, older_ciphertext).unwrap();
        app.selected_entries.insert(app.current_entry_id.unwrap());
        let export = temporary.path().join("export.json");
        app.export_entries(export.clone()).await.unwrap();
        let dto: backend::EntriesDTO =
            serde_json::from_slice(&std::fs::read(export).unwrap()).unwrap();
        assert_eq!(dto.entries[0].content, "Displayed saved body");
        assert!(
            app.update_current_entry_content(
                "Stale edit must not overwrite external change".into()
            )
            .await
            .is_err()
        );
    }

    #[test]
    fn idle_lock_checks_monotonic_and_wall_time_and_can_be_disabled() {
        let mut app = crate::app::test::create_default_app();
        app.settings.security.auto_lock_seconds = 10;
        assert!(!app.security_expired());
        app.security.sessions.insert(
            7,
            UnlockedSession {
                entry: Entry::new(
                    7,
                    chrono::Utc::now(),
                    "Public".into(),
                    "Private".into(),
                    vec![],
                    None,
                ),
                password: "test".to_owned().into(),
                native_metadata: None,
            },
        );
        assert!(!app.security_expired());
        app.security.last_activity = Instant::now() - Duration::from_secs(20);
        assert!(app.security_expired());
        app.record_activity();
        assert!(!app.security_expired());
        app.security.last_wall_activity = SystemTime::now() - Duration::from_secs(20);
        assert!(app.security_expired());
        app.settings.security.auto_lock_seconds = 0;
        assert!(!app.security_expired());
    }
}

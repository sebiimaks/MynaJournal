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

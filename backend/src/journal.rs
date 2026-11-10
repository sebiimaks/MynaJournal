//! Whole-journal encryption over the native storage providers.
//!
//! The sibling header wraps a random journal key. Native records never contain
//! plaintext journal fields while protected. A durable migration intent makes a
//! multi-file replacement recoverable, and is never treated as an ordinary store.
use crate::{
    DataProvider, EncryptedNote, EntriesDTO, Entry, EntryDraft, ModifyEntryError,
    protection::{ExposeSecret, MAX_CIPHERTEXT_BYTES, MAX_PAYLOAD_BYTES, SecretString},
};
use anyhow::{Context, Result, bail, ensure};
use chrono::DateTime;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};
use zeroize::{Zeroize, Zeroizing};

pub const RECORD_VERSION: u32 = 2;
const HEADER_FORMAT: &str = "myna-journal/locked-journal";
const RECORD_FORMAT: &str = "myna-journal/locked-record";
const SCRYPT_FACTOR: u8 = 18;

#[derive(Clone, Debug)]
pub enum JournalBackend {
    #[cfg(feature = "json")]
    Json(PathBuf),
    #[cfg(feature = "sqlite")]
    Sqlite(PathBuf),
    #[cfg(feature = "file")]
    File(PathBuf, crate::FileFormat),
    #[cfg(feature = "vjournal")]
    Vjournal(PathBuf),
}
impl JournalBackend {
    pub fn path(&self) -> &Path {
        match self {
            #[cfg(feature = "json")]
            Self::Json(p) => p,
            #[cfg(feature = "sqlite")]
            Self::Sqlite(p) => p,
            #[cfg(feature = "file")]
            Self::File(p, _) => p,
            #[cfg(feature = "vjournal")]
            Self::Vjournal(p) => p,
        }
    }
    fn at(&self, path: PathBuf) -> Self {
        match self {
            #[cfg(feature = "json")]
            Self::Json(_) => Self::Json(path),
            #[cfg(feature = "sqlite")]
            Self::Sqlite(_) => Self::Sqlite(path),
            #[cfg(feature = "file")]
            Self::File(_, format) => Self::File(path, *format),
            #[cfg(feature = "vjournal")]
            Self::Vjournal(_) => Self::Vjournal(path),
        }
    }
    pub fn header_path(&self) -> PathBuf {
        sibling(self.path(), ".myna-lock.json")
    }
    pub fn pending_path(&self) -> PathBuf {
        sibling(self.path(), ".myna-lock.pending.json")
    }
    pub fn reserved_paths(&self) -> Vec<PathBuf> {
        vec![self.header_path(), self.pending_path(), self.lease_path()]
    }
    fn lease_path(&self) -> PathBuf {
        sibling(self.path(), ".myna-lock.lease")
    }
    fn directory(&self) -> bool {
        match self {
            #[cfg(feature = "json")]
            Self::Json(_) => false,
            #[cfg(feature = "sqlite")]
            Self::Sqlite(_) => false,
            #[cfg(feature = "file")]
            Self::File(..) => true,
            #[cfg(feature = "vjournal")]
            Self::Vjournal(_) => true,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JournalStatus {
    Plain,
    Protected,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalRecord {
    pub entry: Entry,
    pub native_metadata: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    format: String,
    version: u32,
    identity: String,
    wrapped_key: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    format: String,
    journal: String,
    identity: String,
    record: JournalRecord,
    authentication: [u8; 32],
}
#[derive(Serialize)]
struct AuthenticatedFields<'a> {
    format: &'a str,
    journal: &'a str,
    identity: &'a str,
    record: &'a JournalRecord,
}
fn record_authenticator(key: &age::x25519::Identity, payload: &Payload) -> Result<Hmac<Sha256>> {
    let secret = key.to_string();
    let mut derivation = Hmac::<Sha256>::new_from_slice(secret.expose_secret().as_bytes())
        .context("Invalid journal authentication key")?;
    derivation.update(b"MynaJournal/record-authentication-key/v1");
    let key = Zeroizing::new(<[u8; 32]>::from(derivation.finalize().into_bytes()));
    let mut mac = Hmac::<Sha256>::new_from_slice(key.as_ref())
        .context("Invalid journal authentication key")?;
    let bytes = Zeroizing::new(serde_json::to_vec(&AuthenticatedFields {
        format: &payload.format,
        journal: &payload.journal,
        identity: &payload.identity,
        record: &payload.record,
    })?);
    mac.update(&bytes);
    Ok(mac)
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Transition {
    version: u32,
    token: String,
    old_hash: Option<String>,
    new_hash: Option<String>,
    header: Option<Header>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TransactionOwner {
    format: String,
    token: String,
}

enum Native {
    #[cfg(feature = "json")]
    Json(crate::JsonDataProvide),
    #[cfg(feature = "sqlite")]
    Sqlite(crate::SqliteDataProvide),
    #[cfg(feature = "file")]
    File(crate::FileDataProvider),
    #[cfg(feature = "vjournal")]
    Vjournal(crate::VjournalDataProvide),
}
macro_rules! native_call { ($native:expr, $method:ident $(, $arg:expr)*) => { match $native {
    #[cfg(feature="json")] Native::Json(p) => p.$method($($arg),*),
    #[cfg(feature="sqlite")] Native::Sqlite(p) => p.$method($($arg),*),
    #[cfg(feature="file")] Native::File(p) => p.$method($($arg),*),
    #[cfg(feature="vjournal")] Native::Vjournal(p) => p.$method($($arg),*),
}}; }
// Async trait methods return distinct opaque futures; await inside each arm.
macro_rules! native_await { ($native:expr, $method:ident $(, $arg:expr)*) => { match $native {
    #[cfg(feature="json")] Native::Json(p) => p.$method($($arg),*).await,
    #[cfg(feature="sqlite")] Native::Sqlite(p) => p.$method($($arg),*).await,
    #[cfg(feature="file")] Native::File(p) => p.$method($($arg),*).await,
    #[cfg(feature="vjournal")] Native::Vjournal(p) => p.$method($($arg),*).await,
}}; }
impl Native {
    async fn open(backend: &JournalBackend) -> Result<Self> {
        Ok(match backend {
            #[cfg(feature = "json")]
            JournalBackend::Json(p) => Self::Json(crate::JsonDataProvide::new(p.clone())),
            #[cfg(feature = "sqlite")]
            JournalBackend::Sqlite(p) => {
                Self::Sqlite(crate::SqliteDataProvide::from_file(p.clone()).await?)
            }
            #[cfg(feature = "file")]
            JournalBackend::File(p, f) => Self::File(crate::FileDataProvider::new(p.clone(), *f)),
            #[cfg(feature = "vjournal")]
            JournalBackend::Vjournal(p) => {
                Self::Vjournal(crate::VjournalDataProvide::new(p.clone()))
            }
        })
    }
    async fn close(self) {
        match self {
            #[cfg(feature = "sqlite")]
            Self::Sqlite(p) => p.close().await,
            #[allow(unreachable_patterns)]
            _ => {}
        }
    }
}

pub struct JournalProvider {
    backend: JournalBackend,
    native: Option<Native>,
    header: Option<Header>,
    key: Option<age::x25519::Identity>,
    lease: File,
    deleted: HashMap<u32, Option<String>>,
    loaded: bool,
    cache: HashMap<u32, JournalRecord>,
    reload_required: bool,
}
impl JournalProvider {
    pub fn status(backend: &JournalBackend) -> Result<JournalStatus> {
        if exists(&backend.pending_path())? {
            read_transition(backend)?;
            return Ok(JournalStatus::Protected);
        }
        Ok(if read_header(backend)?.is_some() {
            JournalStatus::Protected
        } else {
            JournalStatus::Plain
        })
    }
    pub async fn open(backend: JournalBackend) -> Result<Self> {
        exists(backend.path())?;
        let backend = backend.at(sibling(backend.path(), ""));
        let lease = acquire_lease(&backend)?;
        recover(&backend)?;
        let header = read_header(&backend)?;
        let native = if header.is_none() {
            Some(Native::open(&backend).await?)
        } else {
            None
        };
        Ok(Self {
            backend,
            native,
            header,
            key: None,
            lease,
            deleted: HashMap::new(),
            loaded: false,
            cache: HashMap::new(),
            reload_required: false,
        })
    }
    pub async fn unlock(&mut self, password: SecretString) -> Result<()> {
        self.check()?;
        let header = self.header.as_ref().context("Journal is not protected")?;
        let key = unwrap_key_async(header, password).await?;
        self.check()?;
        let mut native = Native::open(&self.backend).await?;
        let mut markers = 0;
        for entry in native_await!(&mut native, load_all_entries)? {
            markers += usize::from(is_marker(header, &entry));
            decrypt_record(header, &key, entry)?;
        }
        ensure!(
            markers == 1,
            "Journal protection marker is missing or duplicated"
        );
        self.native = Some(native);
        self.key = Some(key);
        self.loaded = false;
        self.cache.clear();
        Ok(())
    }
    pub fn is_locked(&self) -> bool {
        self.header.is_some() && self.key.is_none()
    }
    fn check(&self) -> Result<()> {
        ensure!(
            !exists(&self.backend.pending_path())?,
            "Journal conversion is incomplete; reopen the journal to recover it"
        );
        same_identity(
            &self.lease.metadata()?,
            &fs::symlink_metadata(self.backend.lease_path())?,
        )?;
        let current = read_header(&self.backend)?;
        ensure!(
            serde_json::to_vec(&current)? == serde_json::to_vec(&self.header)?,
            "Journal protection changed externally; reopen the journal"
        );
        Ok(())
    }
    async fn records(&mut self) -> Result<Vec<JournalRecord>> {
        self.check()?;
        if self.header.is_some() {
            preflight(&self.backend)?;
        }
        let native = self.native.as_mut().context("Unlock the journal first")?;
        let entries = native_await!(native, load_all_entries)?;
        let mut records = Vec::with_capacity(entries.len());
        let mut markers = 0;
        for entry in entries {
            if let Some(header) = &self.header {
                let marker = is_marker(header, &entry);
                let record = decrypt_record(
                    header,
                    self.key.as_ref().context("Unlock the journal first")?,
                    entry,
                )?;
                if marker {
                    markers += 1;
                } else {
                    records.push(record);
                }
            } else {
                ensure!(
                    !is_record(&entry),
                    "Journal key header is missing; restore it from the same backup as the journal"
                );
                let native_metadata = if entry.protection.is_none() {
                    native_await!(native, protection_metadata, entry.id)?
                } else {
                    None
                };
                records.push(JournalRecord {
                    entry,
                    native_metadata,
                });
            }
        }
        ensure!(
            self.header.is_none() || markers == 1,
            "Journal protection marker is missing or duplicated"
        );
        self.loaded = true;
        self.cache = if self.header.is_some() {
            records.iter().map(|r| (r.entry.id, r.clone())).collect()
        } else {
            HashMap::new()
        };
        Ok(records)
    }
    async fn ready(&mut self) -> Result<()> {
        self.check()?;
        ensure!(
            !self.reload_required,
            "Journal protection setup refreshed storage; reload the journal before writing"
        );
        if !self.loaded {
            self.load_all_entries().await?;
        }
        if self.header.is_some() {
            preflight(&self.backend)?;
        }
        Ok(())
    }
    fn seal(&self, record: &JournalRecord) -> Result<Entry> {
        match &self.header {
            Some(header) => encrypt_record(
                header,
                self.key.as_ref().context("Unlock the journal first")?,
                record,
            ),
            None => {
                ensure!(
                    !is_record(&record.entry),
                    "Whole-journal ciphertext requires its original key header and a native restore"
                );
                Ok(record.entry.clone())
            }
        }
    }
    async fn convert(
        &mut self,
        records: Vec<JournalRecord>,
        header: Option<Header>,
        key: Option<age::x25519::Identity>,
        old_hash: Option<String>,
    ) -> Result<()> {
        self.reload_required = true;
        self.check()?;
        preflight(&self.backend)?;
        #[cfg(feature = "sqlite")]
        if let Some(Native::Sqlite(provider)) = &self.native {
            provider.validate_journal_conversion().await?;
        }
        ensure!(
            store_hash(self.backend.path())? == old_hash,
            "Journal changed before conversion; reload and try again"
        );
        let token = random_token();
        let work = create_transaction_directory(&self.backend, &token)?;
        let stage_path = work.join("store");
        let stage_backend = self.backend.at(stage_path.clone());
        let mut transition = Transition {
            version: 2,
            token: token.clone(),
            old_hash: old_hash.clone(),
            new_hash: None,
            header: header.clone(),
        };
        write_json(&self.backend.pending_path(), &transition)?;
        let staged = async {
            if self.backend.directory() {
                crate::persistence::create_private_dirs(&stage_path)?;
            }
            #[cfg(feature = "json")]
            if matches!(stage_backend, JournalBackend::Json(_)) {
                write_json(&stage_path, &Vec::<Entry>::new())?;
            }
            let mut stage = Native::open(&stage_backend).await?;
            if let Some(h) = &header {
                let marker = JournalRecord {
                    entry: Entry::new(
                        0,
                        DateTime::UNIX_EPOCH,
                        "Journal marker".into(),
                        String::new(),
                        Vec::new(),
                        None,
                    ),
                    native_metadata: None,
                };
                let marker = encrypt_record_identity(
                    h,
                    key.as_ref().context("Missing journal key")?,
                    &marker,
                    h.identity.clone(),
                )?;
                native_await!(&mut stage, add_entry, EntryDraft::from_entry(marker))?;
            }
            for record in &records {
                let persisted = match &header {
                    Some(h) => {
                        encrypt_record(h, key.as_ref().context("Missing journal key")?, record)?
                    }
                    None => record.entry.clone(),
                };
                let inserted =
                    native_await!(&mut stage, add_entry, EntryDraft::from_entry(persisted))?;
                if header.is_none() && record.native_metadata.is_some() {
                    #[cfg(feature = "file")]
                    #[allow(irrefutable_let_patterns)]
                    if let Native::File(provider) = &mut stage {
                        provider
                            .restore_migration_metadata(
                                inserted,
                                record
                                    .native_metadata
                                    .as_deref()
                                    .context("Missing metadata")?,
                            )
                            .await?;
                        continue;
                    }
                    #[cfg(feature = "vjournal")]
                    #[allow(irrefutable_let_patterns)]
                    if let Native::Vjournal(provider) = &mut stage {
                        provider
                            .restore_migration_metadata(
                                inserted,
                                record
                                    .native_metadata
                                    .as_deref()
                                    .context("Missing metadata")?,
                            )
                            .await?;
                        continue;
                    }
                    native_await!(
                        &mut stage,
                        update_entry_with_metadata,
                        inserted,
                        record.native_metadata.as_deref()
                    )?;
                }
            }
            let actual = native_await!(&mut stage, load_all_entries)?;
            ensure!(
                actual.len() == records.len() + usize::from(header.is_some()),
                "Journal conversion lost entries"
            );
            let mut expected: Vec<_> = records
                .iter()
                .map(|r| {
                    let mut e = r.entry.clone();
                    e.id = 0;
                    serde_json::to_string(&e)
                })
                .collect::<std::result::Result<_, _>>()?;
            let mut found = Vec::new();
            for entry in actual {
                let mut entry = if let Some(h) = &header {
                    let marker = is_marker(h, &entry);
                    let decrypted =
                        decrypt_record(h, key.as_ref().context("Missing journal key")?, entry)?
                            .entry;
                    if marker {
                        continue;
                    }
                    decrypted
                } else {
                    entry
                };
                entry.id = 0;
                found.push(serde_json::to_string(&entry)?);
            }
            expected.sort();
            found.sort();
            ensure!(expected == found, "Journal conversion verification failed");
            stage.close().await;
            Ok::<_, anyhow::Error>(())
        }
        .await;
        if let Err(error) = staged {
            remove_transaction_directory(&work, &token)?;
            fs::remove_file(self.backend.pending_path())?;
            sync_parent(self.backend.path())?;
            return Err(error);
        }
        sync_parent(&stage_path)?;
        if store_hash(self.backend.path())? != old_hash {
            remove_transaction_directory(&work, &token)?;
            fs::remove_file(self.backend.pending_path())?;
            sync_parent(self.backend.path())?;
            bail!("Journal changed during conversion; no changes were published");
        }
        transition.new_hash = Some(store_hash(&stage_path)?.context("Missing staged journal")?);
        write_json(&self.backend.pending_path(), &transition)?;
        if let Some(native) = self.native.take() {
            native.close().await;
        }
        recover(&self.backend)?;
        self.header = header;
        self.key = key;
        self.deleted.clear();
        self.cache.clear();
        self.loaded = false;
        self.native = Some(Native::open(&self.backend).await?);
        Ok(())
    }
}
impl Drop for JournalProvider {
    fn drop(&mut self) {
        self.key = None;
        for record in self.cache.values_mut() {
            record.entry.title.zeroize();
            record.entry.content.zeroize();
            record.entry.tags.zeroize();
            record.entry.priority.zeroize();
            record.entry.mood.zeroize();
            record.entry.journal_prompt.zeroize();
            record.entry.date = DateTime::UNIX_EPOCH;
            record.native_metadata.zeroize();
        }
        for metadata in self.deleted.values_mut() {
            metadata.zeroize();
        }
        self.native = None;
        let _ = self.lease.unlock();
    }
}

impl DataProvider for JournalProvider {
    async fn close_for_move(&mut self) -> Result<()> {
        self.check()?;
        ensure!(!self.reload_required, "Reopen the journal before moving it");
        if let Some(native) = self.native.take() {
            native.close().await;
        }
        self.key = None;
        // Keep cached records for Drop to zeroize when the caller releases this provider.
        self.loaded = false;
        self.reload_required = true;
        Ok(())
    }
    fn is_journal_protected(&self) -> bool {
        self.header.is_some()
    }
    fn journal_reload_required(&self) -> bool {
        self.reload_required
    }
    fn retain_restore_metadata(&mut self, ids: &[u32]) {
        self.deleted.retain(|id, _| ids.contains(id));
        if let Some(native) = self.native.as_mut() {
            native_call!(native, retain_restore_metadata, ids);
        }
    }
    async fn load_all_entries(&mut self) -> Result<Vec<Entry>> {
        self.reload_required = true;
        if self.header.is_some() {
            let entries = self.records().await?.into_iter().map(|r| r.entry).collect();
            self.reload_required = false;
            return Ok(entries);
        }
        self.check()?;
        let entries = native_await!(
            self.native.as_mut().context("Journal is not open")?,
            load_all_entries
        )?;
        ensure!(
            entries.iter().all(|entry| !is_record(entry)),
            "Journal key header is missing; restore it from the same backup as the journal"
        );
        self.loaded = true;
        self.reload_required = false;
        Ok(entries)
    }
    async fn journal_records(&mut self) -> Result<Vec<JournalRecord>> {
        self.reload_required = true;
        self.records().await
    }
    async fn enable_journal_protection(
        &mut self,
        password: SecretString,
        supplied: Option<Vec<JournalRecord>>,
    ) -> Result<()> {
        ensure!(self.header.is_none(), "Journal is already protected");
        self.check()?;
        if !self.loaded {
            self.load_all_entries().await?;
        }
        self.reload_required = true;
        let old_hash = store_hash(self.backend.path())?;
        let current = self.records().await?;
        let records = supplied.unwrap_or_else(|| current.clone());
        ensure!(
            current.len() == records.len(),
            "Journal entries changed before protection"
        );
        let mut ids = HashSet::new();
        for record in &records {
            ensure!(
                ids.insert(record.entry.id),
                "Duplicate entry in journal conversion"
            );
            let old = current
                .iter()
                .find(|r| r.entry.id == record.entry.id)
                .context("Journal entries changed before protection")?;
            ensure!(
                record.entry.protection.is_none(),
                "Unlock every individually protected entry before enabling journal protection"
            );
            if old.entry.protection.is_none() {
                ensure!(
                    old.entry == record.entry && old.native_metadata == record.native_metadata,
                    "Journal changed before protection"
                );
            } else {
                ensure!(
                    old.entry.title == record.entry.title && old.entry.date == record.entry.date,
                    "Protected entry identity changed"
                );
            }
        }
        let (key, header) = tokio::task::spawn_blocking(move || {
            let key = age::x25519::Identity::generate();
            let header = wrap_key(&key, &password)?;
            Ok::<_, anyhow::Error>((key, header))
        })
        .await??;
        self.convert(records, Some(header), Some(key), old_hash)
            .await
    }
    async fn change_journal_password(
        &mut self,
        current: SecretString,
        new: SecretString,
    ) -> Result<()> {
        self.check()?;
        let old = self
            .header
            .as_ref()
            .context("Journal is not protected")?
            .clone();
        let header = tokio::task::spawn_blocking(move || {
            let key = unwrap_key(&old, &current)?;
            wrap_key(&key, &new)
        })
        .await??;
        self.check()?;
        write_json(&self.backend.header_path(), &header)?;
        self.header = Some(header);
        Ok(())
    }
    async fn disable_journal_protection(&mut self, password: SecretString) -> Result<()> {
        let key = unwrap_key_async(
            self.header.as_ref().context("Journal is not protected")?,
            password,
        )
        .await?;
        self.check()?;
        self.reload_required = true;
        self.key = Some(key);
        if self.native.is_none() {
            self.native = Some(Native::open(&self.backend).await?);
        }
        let old_hash = store_hash(self.backend.path())?;
        let records = self.records().await?;
        self.convert(records, None, None, old_hash).await
    }
    async fn add_entry(
        &mut self,
        entry: EntryDraft,
    ) -> std::result::Result<Entry, ModifyEntryError> {
        self.ready().await?;
        let mut logical = Entry::from_draft(0, entry);
        normalize_native_entry(&self.backend, &mut logical)?;
        let stored = self.seal(&JournalRecord {
            entry: logical.clone(),
            native_metadata: None,
        })?;
        let result = native_await!(
            self.native.as_mut().context("Unlock the journal first")?,
            add_entry,
            EntryDraft::from_entry(stored)
        )?;
        if self.header.is_none() {
            return Ok(result);
        }
        logical.id = result.id;
        self.cache.insert(
            logical.id,
            JournalRecord {
                entry: logical.clone(),
                native_metadata: None,
            },
        );
        Ok(logical)
    }
    async fn restore_entry(
        &mut self,
        mut entry: Entry,
    ) -> std::result::Result<Entry, ModifyEntryError> {
        normalize_native_entry(&self.backend, &mut entry)?;
        self.ready().await?;
        let metadata = self.deleted.get(&entry.id).cloned().flatten();
        let stored = self.seal(&JournalRecord {
            entry: entry.clone(),
            native_metadata: metadata.clone(),
        })?;
        let restored = native_await!(
            self.native.as_mut().context("Unlock the journal first")?,
            restore_entry,
            stored
        )?;
        self.deleted.remove(&entry.id);
        if self.header.is_some() {
            self.cache.insert(
                entry.id,
                JournalRecord {
                    entry: entry.clone(),
                    native_metadata: metadata,
                },
            );
        }
        Ok(if self.header.is_some() {
            entry
        } else {
            restored
        })
    }
    async fn remove_entry(&mut self, id: u32) -> Result<()> {
        if self.header.is_none() {
            self.ready().await?;
            return native_await!(
                self.native.as_mut().context("Journal is not open")?,
                remove_entry,
                id
            );
        }
        self.ready().await?;
        let record = self
            .cache
            .get(&id)
            .cloned()
            .context("Entry no longer exists")?;
        native_await!(
            self.native.as_mut().context("Unlock the journal first")?,
            remove_entry,
            id
        )?;
        self.cache.remove(&id);
        self.deleted.insert(id, record.native_metadata);
        Ok(())
    }
    async fn update_entry(&mut self, entry: Entry) -> std::result::Result<Entry, ModifyEntryError> {
        self.update_entry_with_metadata(entry, None).await
    }
    async fn protection_metadata(&mut self, id: u32) -> Result<Option<String>> {
        if self.header.is_none() {
            self.ready().await?;
            return native_await!(
                self.native.as_mut().context("Journal is not open")?,
                protection_metadata,
                id
            );
        }
        self.ready().await?;
        Ok(self
            .cache
            .get(&id)
            .context("Entry no longer exists")?
            .native_metadata
            .clone())
    }
    async fn update_entry_with_metadata(
        &mut self,
        entry: Entry,
        metadata: Option<&str>,
    ) -> std::result::Result<Entry, ModifyEntryError> {
        let mut entry = entry;
        normalize_native_entry(&self.backend, &mut entry)?;
        if self.header.is_none() {
            if is_record(&entry) {
                return Err(anyhow::anyhow!(
                    "Whole-journal ciphertext requires its original key header and a native restore"
                )
                .into());
            }
            self.ready().await?;
            return native_await!(
                self.native.as_mut().context("Unlock the journal first")?,
                update_entry_with_metadata,
                entry,
                metadata
            );
        }
        self.ready().await?;
        let previous = self
            .cache
            .get(&entry.id)
            .cloned()
            .context("Entry no longer exists")?;
        let record = JournalRecord {
            entry: entry.clone(),
            native_metadata: metadata.map(str::to_owned).or(previous.native_metadata),
        };
        let stored = self.seal(&record)?;
        native_await!(
            self.native.as_mut().context("Unlock the journal first")?,
            update_entry,
            stored
        )?;
        self.cache.insert(entry.id, record);
        Ok(entry)
    }
    async fn get_export_object(&mut self, ids: &[u32]) -> Result<EntriesDTO> {
        ensure!(
            self.header.is_none(),
            "Protected journals require a native backup including their key header; plaintext export requires explicit confirmation"
        );
        self.ready().await?;
        native_await!(
            self.native.as_mut().context("Unlock the journal first")?,
            get_export_object,
            ids
        )
    }
    async fn import_entries(&mut self, dto: EntriesDTO) -> Result<()> {
        dto.validate()?;
        ensure!(
            dto.entries.iter().all(|entry| entry
                .protection
                .as_ref()
                .is_none_or(|p| p.version != RECORD_VERSION)),
            "Whole-journal ciphertext requires its original key header and a native restore"
        );
        self.ready().await?;
        let dto = if self.header.is_some() {
            let mut encrypted = Vec::new();
            for draft in dto.entries {
                let mut entry = Entry::from_draft(0, draft);
                normalize_native_entry(&self.backend, &mut entry)?;
                encrypted.push(EntryDraft::from_entry(self.seal(&JournalRecord {
                    entry,
                    native_metadata: None,
                })?));
            }
            EntriesDTO::new(encrypted)
        } else {
            dto
        };
        native_await!(
            self.native.as_mut().context("Unlock the journal first")?,
            import_entries,
            dto
        )?;
        self.loaded = false;
        Ok(())
    }
    async fn assign_priority_to_entries(&mut self, priority: u32) -> Result<()> {
        if self.header.is_none() {
            self.ready().await?;
            return native_await!(
                self.native.as_mut().context("Journal is not open")?,
                assign_priority_to_entries,
                priority
            );
        }
        for mut entry in self.load_all_entries().await? {
            if entry.priority.is_none() && entry.protection.is_none() {
                entry.priority = Some(priority);
                self.update_entry(entry).await?;
            }
        }
        Ok(())
    }
}

fn normalize_native_entry(
    backend: &JournalBackend,
    entry: &mut Entry,
) -> std::result::Result<(), ModifyEntryError> {
    if entry.title.is_empty() {
        return Err(ModifyEntryError::ValidationError(
            "Entry title can't be empty".into(),
        ));
    }
    #[cfg(feature = "vjournal")]
    if matches!(backend, JournalBackend::Vjournal(_)) && entry.protection.is_none() {
        entry.priority = match entry.priority {
            None | Some(0) => None,
            Some(value @ 1..=9) => Some(value),
            Some(_) => {
                return Err(ModifyEntryError::ValidationError(
                    "iCalendar priorities must be 1 through 9".into(),
                ));
            }
        };
        entry.date = entry
            .date
            .date_naive()
            .and_hms_opt(0, 0, 0)
            .expect("midnight is valid")
            .and_utc();
    }
    let _ = (backend, entry);
    Ok(())
}

fn is_record(entry: &Entry) -> bool {
    entry
        .protection
        .as_ref()
        .is_some_and(|p| p.version == RECORD_VERSION)
}
fn wrap_key(key: &age::x25519::Identity, password: &SecretString) -> Result<Header> {
    ensure!(
        !password.expose_secret().is_empty() && password.expose_secret().len() <= 4096,
        "Enter a nonempty password of at most 4096 bytes"
    );
    let mut recipient = age::scrypt::Recipient::new(password.clone());
    recipient.set_work_factor(SCRYPT_FACTOR);
    let wrapped_key =
        age::encrypt_and_armor(&recipient, key.to_string().expose_secret().as_bytes())
            .context("Encrypting journal key")?;
    Ok(Header {
        format: HEADER_FORMAT.into(),
        version: 1,
        identity: key.to_public().to_string(),
        wrapped_key,
    })
}
async fn unwrap_key_async(
    header: &Header,
    password: SecretString,
) -> Result<age::x25519::Identity> {
    let header = header.clone();
    tokio::task::spawn_blocking(move || unwrap_key(&header, &password)).await?
}
fn unwrap_key(header: &Header, password: &SecretString) -> Result<age::x25519::Identity> {
    ensure!(
        password.expose_secret().len() <= 4096,
        "Password is too long"
    );
    let decryptor = age::Decryptor::new(age::armor::ArmoredReader::new(
        header.wrapped_key.as_bytes(),
    ))
    .context("Invalid journal key")?;
    ensure!(decryptor.is_scrypt(), "Invalid journal key protection");
    let mut identity = age::scrypt::Identity::new(password.clone());
    identity.set_max_work_factor(SCRYPT_FACTOR);
    let reader = decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(|_| anyhow::anyhow!("The password is incorrect or the journal key is damaged"))?;
    let mut bytes = Zeroizing::new(Vec::new());
    reader.take(257).read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= 256, "Invalid journal key");
    let key: age::x25519::Identity = std::str::from_utf8(&bytes)?
        .parse()
        .map_err(|_| anyhow::anyhow!("Invalid journal key"))?;
    ensure!(
        key.to_public().to_string() == header.identity,
        "Journal key identity does not match"
    );
    Ok(key)
}
fn encrypt_record(
    header: &Header,
    key: &age::x25519::Identity,
    record: &JournalRecord,
) -> Result<Entry> {
    encrypt_record_identity(
        header,
        key,
        record,
        age::x25519::Identity::generate().to_public().to_string(),
    )
}
fn is_marker(header: &Header, entry: &Entry) -> bool {
    entry
        .protection
        .as_ref()
        .is_some_and(|p| p.identity == header.identity)
}
fn encrypt_record_identity(
    header: &Header,
    key: &age::x25519::Identity,
    record: &JournalRecord,
    identity: String,
) -> Result<Entry> {
    ensure!(
        record.entry.protection.is_none(),
        "Individually protected entries must be unlocked before adding them to a protected journal"
    );
    let mut payload = Payload {
        format: RECORD_FORMAT.into(),
        journal: header.identity.clone(),
        identity: identity.clone(),
        record: record.clone(),
        authentication: [0; 32],
    };
    payload.authentication = record_authenticator(key, &payload)?
        .finalize()
        .into_bytes()
        .into();
    let plaintext = Zeroizing::new(serde_json::to_vec(&payload)?);
    ensure!(
        plaintext.len() <= MAX_PAYLOAD_BYTES,
        "Journal entry exceeds the encrypted size limit"
    );
    let ciphertext =
        age::encrypt_and_armor(&key.to_public(), &plaintext).context("Encrypting journal entry")?;
    ensure!(
        ciphertext.len() <= MAX_CIPHERTEXT_BYTES,
        "Journal entry exceeds the encrypted size limit"
    );
    Ok(Entry::new(
        record.entry.id,
        DateTime::UNIX_EPOCH,
        "Locked entry".into(),
        String::new(),
        Vec::new(),
        None,
    )
    .with_protection(EncryptedNote {
        version: RECORD_VERSION,
        identity,
        ciphertext,
    }))
}
fn decrypt_record(
    header: &Header,
    key: &age::x25519::Identity,
    entry: Entry,
) -> Result<JournalRecord> {
    entry.validate_protection()?;
    ensure!(
        is_record(&entry) && entry.title == "Locked entry" && entry.date == DateTime::UNIX_EPOCH,
        "Protected journal contains an unencrypted or malformed entry"
    );
    let envelope = entry
        .protection
        .as_ref()
        .context("Missing encrypted entry")?;
    let decryptor = age::Decryptor::new(age::armor::ArmoredReader::new(
        envelope.ciphertext.as_bytes(),
    ))
    .context("Invalid encrypted entry")?;
    ensure!(!decryptor.is_scrypt(), "Invalid journal entry key type");
    let reader = decryptor
        .decrypt(std::iter::once(key as &dyn age::Identity))
        .context("Journal entry is damaged or belongs to another journal")?;
    let mut plaintext = Zeroizing::new(Vec::new());
    reader
        .take(MAX_PAYLOAD_BYTES as u64 + 1)
        .read_to_end(&mut plaintext)?;
    ensure!(
        plaintext.len() <= MAX_PAYLOAD_BYTES,
        "Encrypted journal entry is too large"
    );
    let mut payload: Payload =
        serde_json::from_slice(&plaintext).context("Invalid encrypted journal entry")?;
    record_authenticator(key, &payload)?
        .verify_slice(&payload.authentication)
        .context("Journal entry authentication failed")?;
    ensure!(
        payload.format == RECORD_FORMAT
            && payload.journal == header.identity
            && payload.identity == envelope.identity,
        "Journal entry identity does not match"
    );
    ensure!(
        payload.record.entry.protection.is_none(),
        "Nested entry protection is unsupported"
    );
    payload.record.entry.id = entry.id;
    Ok(payload.record)
}
fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = fs::canonicalize(parent).unwrap_or_else(|_| parent.to_owned());
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(suffix);
    parent.join(name)
}
fn exists(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            ensure!(
                !metadata.file_type().is_symlink(),
                "Journal storage symlinks are not supported: {}",
                path.display()
            );
            Ok(true)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}
fn read_bounded(path: &Path, limit: usize) -> Result<Vec<u8>> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() <= limit as u64,
        "Invalid journal protection file"
    );
    let mut bytes = Vec::new();
    File::open(path)?
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= limit, "Journal protection file is too large");
    Ok(bytes)
}
fn validate_header(header: &Header) -> Result<()> {
    ensure!(
        header.format == HEADER_FORMAT && header.version == 1,
        "Unsupported journal protection version"
    );
    ensure!(
        header.identity.parse::<age::x25519::Recipient>().is_ok()
            && header.wrapped_key.len() <= 8192,
        "Invalid journal key header"
    );
    Ok(())
}
fn read_header(backend: &JournalBackend) -> Result<Option<Header>> {
    let path = backend.header_path();
    if !exists(&path)? {
        return Ok(None);
    }
    let header: Header = serde_json::from_slice(&read_bounded(&path, 16384)?)?;
    validate_header(&header)?;
    Ok(Some(header))
}
fn write_json(path: &Path, value: &impl Serialize) -> Result<()> {
    let parent = path.parent().context("Missing journal parent directory")?;
    crate::persistence::create_private_dirs(parent)?;
    if exists(path)? {
        ensure!(
            fs::metadata(path)?.is_file(),
            "Journal protection path is not a file"
        );
    }
    let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
    serde_json::to_writer(&mut tmp, value)?;
    tmp.flush()?;
    tmp.as_file().sync_all()?;
    tmp.persist(path)?;
    sync_parent(path)
}
fn sync_parent(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path.parent().context("Missing journal parent directory")?)?.sync_all()?;
    Ok(())
}
fn random_token() -> String {
    use rand::RngCore;
    let mut bytes = [0_u8; 16];
    rand::thread_rng().fill_bytes(&mut bytes);
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn same_identity(a: &fs::Metadata, b: &fs::Metadata) -> Result<()> {
    ensure!(
        a.is_file() && b.is_file(),
        "Journal lock is not a regular file"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            a.dev() == b.dev() && a.ino() == b.ino(),
            "Journal lock was replaced"
        );
    }
    Ok(())
}
fn acquire_lease(backend: &JournalBackend) -> Result<File> {
    let path = backend.lease_path();
    crate::persistence::create_private_dirs(
        path.parent().context("Invalid journal storage path")?,
    )?;
    if exists(&path)? {
        ensure!(
            fs::metadata(&path)?.is_file(),
            "Journal lock is not a regular file"
        );
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(&path)?;
    file.try_lock()
        .context("Journal is in use; close other instances before opening it")?;
    same_identity(&file.metadata()?, &fs::symlink_metadata(path)?)?;
    Ok(file)
}
/// Holds the journal/header migration lease without creating or opening a native store.
pub struct JournalLease(File);
impl JournalLease {
    pub fn acquire(backend: &JournalBackend) -> Result<Self> {
        let lease = Self(acquire_lease(backend)?);
        ensure!(
            !exists(&backend.pending_path())?,
            "Journal conversion is incomplete; open the journal to recover it before backing up"
        );
        Ok(lease)
    }
}
impl Drop for JournalLease {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}
fn store_hash(path: &Path) -> Result<Option<String>> {
    if !exists(path)? {
        return Ok(None);
    }
    let mut hasher = Sha256::new();
    hash_path(path, &mut hasher)?;
    Ok(Some(format!("{:x}", hasher.finalize())))
}
fn hash_path(path: &Path, hasher: &mut Sha256) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    ensure!(
        !metadata.file_type().is_symlink(),
        "Journal storage contains a symlink"
    );
    if metadata.is_file() {
        hasher.update(b"file");
        hasher.update(metadata.len().to_le_bytes());
        let mut f = File::open(path)?;
        same_identity(&metadata, &f.metadata()?)?;
        let mut count = 0_u64;
        let mut buf = [0; 65536];
        loop {
            let n = f.read(&mut buf)?;
            if n == 0 {
                break;
            }
            count += n as u64;
            hasher.update(&buf[..n]);
        }
        ensure!(
            count == metadata.len() && f.metadata()?.modified()? == metadata.modified()?,
            "Journal changed while checksumming"
        );
        same_identity(&metadata, &fs::symlink_metadata(path)?)?;
    } else {
        ensure!(metadata.is_dir(), "Unsupported object in journal storage");
        hasher.update(b"directory");
        let mut files = fs::read_dir(path)?
            .map(|e| e.map(|e| e.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        files.sort();
        let mut retained = Vec::new();
        for file in files {
            if !is_native_lock(&file)? {
                retained.push(file);
            }
        }
        let files = retained;
        hasher.update((files.len() as u64).to_le_bytes());
        for file in files {
            let name = file
                .file_name()
                .context("Invalid journal filename")?
                .as_encoded_bytes();
            hasher.update((name.len() as u64).to_le_bytes());
            hasher.update(name);
            hash_path(&file, hasher)?;
        }
    }
    Ok(())
}
fn remove_store(path: &Path) -> Result<()> {
    if !exists(path)? {
        return Ok(());
    }
    if fs::symlink_metadata(path)?.is_dir() {
        fs::remove_dir_all(path)?;
    } else {
        fs::remove_file(path)?;
    }
    sync_parent(path)
}
fn transaction_directory(backend: &JournalBackend, token: &str) -> PathBuf {
    sibling(backend.path(), &format!(".myna-transaction-{token}"))
}
fn create_transaction_directory(backend: &JournalBackend, token: &str) -> Result<PathBuf> {
    let path = transaction_directory(backend, token);
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(&path)
        .context("Creating private journal migration directory")?;
    write_json(
        &path.join("owner.json"),
        &TransactionOwner {
            format: "myna-journal/migration-directory/v1".into(),
            token: token.into(),
        },
    )?;
    sync_parent(&path)?;
    Ok(path)
}
fn validate_transaction_directory(path: &Path, token: &str) -> Result<fs::Metadata> {
    let before = fs::symlink_metadata(path)?;
    ensure!(
        before.is_dir() && !before.file_type().is_symlink(),
        "Journal migration directory was replaced"
    );
    let owner: TransactionOwner =
        serde_json::from_slice(&read_bounded(&path.join("owner.json"), 2048)?)?;
    ensure!(
        owner.format == "myna-journal/migration-directory/v1" && owner.token == token,
        "Journal migration directory has no matching ownership marker; preserve it for manual recovery"
    );
    let after = fs::symlink_metadata(path)?;
    ensure!(
        after.is_dir() && !after.file_type().is_symlink(),
        "Journal migration directory was replaced"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        ensure!(
            before.dev() == after.dev() && before.ino() == after.ino(),
            "Journal migration directory was replaced"
        );
    }
    Ok(after)
}
fn remove_transaction_directory(path: &Path, token: &str) -> Result<()> {
    if !exists(path)? {
        return Ok(());
    }
    validate_transaction_directory(path, token)?;
    fs::remove_dir_all(path)?;
    sync_parent(path)
}
fn read_transition(backend: &JournalBackend) -> Result<Transition> {
    let tx: Transition = serde_json::from_slice(&read_bounded(&backend.pending_path(), 32768)?)?;
    ensure!(
        matches!(tx.version, 1 | 2)
            && tx.token.len() == 32
            && tx.token.bytes().all(|b| b.is_ascii_hexdigit()),
        "Invalid journal conversion record"
    );
    ensure!(
        tx.new_hash.as_ref().is_none_or(|h| h.len() == 64)
            && tx.old_hash.as_ref().is_none_or(|h| h.len() == 64),
        "Invalid journal conversion checksum"
    );
    if let Some(h) = &tx.header {
        validate_header(h)?;
    }
    Ok(tx)
}
fn recover(backend: &JournalBackend) -> Result<()> {
    if !exists(&backend.pending_path())? {
        return Ok(());
    }
    let tx = read_transition(backend)?;
    let work = (tx.version == 2).then(|| transaction_directory(backend, &tx.token));
    if let Some(work) = &work
        && exists(work)?
    {
        validate_transaction_directory(work, &tx.token)?;
    }
    let stage = work.as_ref().map_or_else(
        || sibling(backend.path(), &format!(".myna-stage-{}", tx.token)),
        |work| work.join("store"),
    );
    let previous = sibling(backend.path(), &format!(".myna-previous-{}", tx.token));
    let current = store_hash(backend.path())?;
    let Some(new_hash) = &tx.new_hash else {
        ensure!(
            tx.version == 2,
            "An older preview interrupted this conversion; preserve the journal and staging files for manual recovery"
        );
        ensure!(
            current == tx.old_hash && !exists(&previous)?,
            "Journal changed during an interrupted conversion; manual recovery is required"
        );
        remove_transaction_directory(
            work.as_ref().expect("version 2 has a migration directory"),
            &tx.token,
        )?;
        fs::remove_file(backend.pending_path())?;
        sync_parent(backend.path())?;
        return Ok(());
    };
    if current.as_ref() != Some(new_hash) {
        ensure!(
            store_hash(&stage)?.as_ref() == Some(new_hash),
            "Incomplete journal conversion: staged data is missing or changed; preserve the store and conversion files for recovery"
        );
        if current.is_some() {
            ensure!(
                current == tx.old_hash && !exists(&previous)?,
                "Journal changed during conversion; refusing to overwrite it"
            );
            fs::rename(backend.path(), &previous)?;
            sync_parent(backend.path())?;
        } else {
            ensure!(
                tx.old_hash.is_none() || store_hash(&previous)? == tx.old_hash,
                "Original journal is missing during conversion"
            );
        }
        fs::rename(&stage, backend.path())?;
        if work.is_some() {
            sync_parent(&stage)?;
        }
        sync_parent(backend.path())?;
    }
    match &tx.header {
        Some(header) => write_json(&backend.header_path(), header)?,
        None => {
            if exists(&backend.header_path())? {
                fs::remove_file(backend.header_path())?;
                sync_parent(backend.path())?;
            }
        }
    }
    if exists(&previous)? {
        ensure!(
            store_hash(&previous)? == tx.old_hash,
            "Original journal changed during conversion cleanup"
        );
        remove_store(&previous)?;
    }
    if exists(&stage)? {
        ensure!(
            store_hash(&stage)?.as_ref() == Some(new_hash),
            "Unexpected staged journal data"
        );
        remove_store(&stage)?;
    }
    if let Some(work) = &work {
        remove_transaction_directory(work, &tx.token)?;
    }
    fs::remove_file(backend.pending_path())?;
    sync_parent(backend.path())?;
    Ok(())
}
fn is_native_lock(path: &Path) -> Result<bool> {
    if path
        .file_name()
        .is_some_and(|name| name == ".tui-journal.lock")
    {
        let metadata = fs::symlink_metadata(path)?;
        ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink() && metadata.len() == 0,
            "Journal native lock must be an empty regular file"
        );
        return Ok(true);
    }
    Ok(false)
}
fn preflight(backend: &JournalBackend) -> Result<()> {
    if !exists(backend.path())? {
        return Ok(());
    }
    if !backend.directory() {
        ensure!(
            fs::symlink_metadata(backend.path())?.is_file(),
            "Journal store must be a regular file"
        );
        #[cfg(feature = "json")]
        if matches!(backend, JournalBackend::Json(_)) {
            let bytes = fs::read(backend.path())?;
            if !bytes.is_empty() {
                let value: serde_json::Value = serde_json::from_slice(&bytes)?;
                let entries = value
                    .as_array()
                    .or_else(|| value.get("entries").and_then(serde_json::Value::as_array))
                    .context("Invalid JSON journal")?;
                for entry in entries {
                    let object = entry.as_object().context("Invalid JSON journal entry")?;
                    ensure!(
                        object.keys().all(|key| matches!(
                            key.as_str(),
                            "id" | "date"
                                | "title"
                                | "content"
                                | "tags"
                                | "priority"
                                | "protection"
                                | "mood"
                                | "journal_prompt"
                        )),
                        "Journal conversion cannot discard unknown JSON entry fields"
                    );
                }
            }
        }
        return Ok(());
    }
    #[cfg(feature = "vjournal")]
    let mut calendar_ids = HashSet::new();
    for item in fs::read_dir(backend.path())? {
        let path = item?.path();
        if is_native_lock(&path)? {
            continue;
        }
        let metadata = fs::symlink_metadata(&path)?;
        ensure!(
            metadata.is_file() && !metadata.file_type().is_symlink(),
            "Journal conversion requires a directory containing only journal files; unsupported item: {}",
            path.display()
        );
        match backend {
            #[cfg(feature = "file")]
            JournalBackend::File(..) => crate::file::validate_journal_conversion_file(&path)
                .with_context(|| format!("Conversion blocked by {}", path.display()))?,
            #[cfg(feature = "vjournal")]
            JournalBackend::Vjournal(..) => {
                for uid in crate::vjournal::validate_journal_conversion_file(&path)
                    .with_context(|| format!("Conversion blocked by {}", path.display()))?
                {
                    ensure!(
                        calendar_ids.insert(uid),
                        "Journal has duplicate recurrence masters across files"
                    );
                }
            }
            #[allow(unreachable_patterns)]
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn password() -> SecretString {
        SecretString::from("journal test password".to_string())
    }
    fn backends(root: &Path) -> Vec<JournalBackend> {
        vec![
            #[cfg(feature = "json")]
            JournalBackend::Json(root.join("entries.json")),
            #[cfg(feature = "sqlite")]
            JournalBackend::Sqlite(root.join("entries.db")),
            #[cfg(feature = "file")]
            JournalBackend::File(root.join("files"), crate::FileFormat::Plain),
            #[cfg(feature = "vjournal")]
            JournalBackend::Vjournal(root.join("calendar")),
        ]
    }
    fn draft() -> EntryDraft {
        EntryDraft::new(
            DateTime::parse_from_rfc3339("2024-03-25T00:00:00Z")
                .unwrap()
                .to_utc(),
            "secret title canary".into(),
            vec!["secret-tag-canary".into()],
            Some(2),
        )
        .with_content("secret body canary".into())
    }
    fn bytes_under(path: &Path) -> Vec<u8> {
        let mut out = Vec::new();
        if path.is_dir() {
            for file in fs::read_dir(path).unwrap() {
                out.extend(bytes_under(&file.unwrap().path()));
            }
        } else {
            out.extend(fs::read(path).unwrap());
        }
        out
    }
    #[tokio::test]
    async fn move_shutdown_blocks_further_access_and_keeps_lease_until_drop() {
        let dir = tempfile::tempdir().unwrap();
        for backend in backends(dir.path()) {
            let mut provider = JournalProvider::open(backend.clone()).await.unwrap();
            provider.add_entry(draft()).await.unwrap();
            provider
                .enable_journal_protection(password(), None)
                .await
                .unwrap();
            provider.load_all_entries().await.unwrap();
            provider.close_for_move().await.unwrap();
            assert!(provider.journal_reload_required());
            assert!(provider.is_locked());
            assert!(provider.load_all_entries().await.is_err());
            assert!(provider.add_entry(draft()).await.is_err());
            assert!(JournalLease::acquire(&backend).is_err());
            drop(provider);

            let mut reopened = JournalProvider::open(backend).await.unwrap();
            reopened.unlock(password()).await.unwrap();
            let entries = reopened.load_all_entries().await.unwrap();
            assert_eq!(entries.len(), 1);
            assert_eq!(entries[0].content, "secret body canary");
            reopened.close_for_move().await.unwrap();
        }
    }
    #[tokio::test]
    async fn all_native_backends_encrypt_every_field_and_support_changes_undo_and_unprotection() {
        let dir = tempfile::tempdir().unwrap();
        for backend in backends(dir.path()) {
            let mut provider = JournalProvider::open(backend.clone()).await.unwrap();
            let original = provider.add_entry(draft()).await.unwrap();
            let original_metadata = provider.protection_metadata(original.id).await.unwrap();
            provider
                .enable_journal_protection(password(), None)
                .await
                .unwrap();
            assert!(provider.is_journal_protected());
            for canary in [
                "secret title canary",
                "secret-tag-canary",
                "secret body canary",
                "2024-03-25",
            ] {
                assert!(
                    !String::from_utf8_lossy(&bytes_under(dir.path())).contains(canary),
                    "plaintext leaked for {backend:?}"
                );
            }
            drop(provider);
            let mut provider = JournalProvider::open(backend.clone()).await.unwrap();
            assert!(provider.is_locked());
            assert!(provider.load_all_entries().await.is_err());
            assert!(
                provider
                    .unlock(SecretString::from("wrong".to_string()))
                    .await
                    .is_err()
            );
            provider.unlock(password()).await.unwrap();
            let mut loaded = provider.load_all_entries().await.unwrap().remove(0);
            let mut expected = original.clone();
            expected.id = loaded.id;
            assert_eq!(loaded, expected);
            assert_eq!(
                provider.protection_metadata(loaded.id).await.unwrap(),
                original_metadata
            );
            loaded.content = "updated private body".into();
            provider.update_entry(loaded.clone()).await.unwrap();
            provider.remove_entry(loaded.id).await.unwrap();
            provider.restore_entry(loaded.clone()).await.unwrap();
            assert_eq!(
                provider.load_all_entries().await.unwrap(),
                vec![loaded.clone()]
            );
            assert!(provider.get_export_object(&[loaded.id]).await.is_err());
            provider
                .disable_journal_protection(password())
                .await
                .unwrap();
            assert!(!provider.is_journal_protected());
            assert!(!backend.header_path().exists());
            let mut unprotected = provider.load_all_entries().await.unwrap().remove(0);
            let _unprotected_id = unprotected.id;
            unprotected.id = loaded.id;
            assert_eq!(unprotected, loaded);
            #[cfg(feature = "vjournal")]
            if matches!(backend, JournalBackend::Vjournal(_)) {
                let after: serde_json::Value = serde_json::from_str(
                    &provider
                        .protection_metadata(_unprotected_id)
                        .await
                        .unwrap()
                        .unwrap(),
                )
                .unwrap();
                let before: serde_json::Value =
                    serde_json::from_str(original_metadata.as_ref().unwrap()).unwrap();
                let uids = |value: &serde_json::Value| {
                    value["document"]
                        .as_str()
                        .unwrap()
                        .lines()
                        .filter(|line| line.starts_with("UID:"))
                        .map(str::to_owned)
                        .collect::<Vec<_>>()
                };
                assert_eq!(uids(&before), uids(&after));
            }
            #[cfg(feature = "file")]
            if matches!(backend, JournalBackend::File(..)) {
                let after: serde_json::Value = serde_json::from_str(
                    &provider
                        .protection_metadata(_unprotected_id)
                        .await
                        .unwrap()
                        .unwrap(),
                )
                .unwrap();
                let before: serde_json::Value =
                    serde_json::from_str(original_metadata.as_ref().unwrap()).unwrap();
                let before = before["document"]
                    .as_str()
                    .unwrap()
                    .lines()
                    .find(|line| line.starts_with("created:"))
                    .unwrap();
                let after = after["document"]
                    .as_str()
                    .unwrap()
                    .lines()
                    .find(|line| line.starts_with("created:"))
                    .unwrap();
                assert_eq!(before, after);
            }
            drop(provider);
            remove_store(backend.path()).unwrap();
        }
    }
    async fn external_edit(backend: &JournalBackend) {
        #[cfg(feature = "sqlite")]
        #[allow(irrefutable_let_patterns)]
        if let JournalBackend::Sqlite(path) = backend {
            use sqlx::Connection;
            let options = sqlx::sqlite::SqliteConnectOptions::new().filename(path);
            let mut connection = sqlx::SqliteConnection::connect_with(&options)
                .await
                .unwrap();
            sqlx::query("UPDATE entries SET title = 'external change'")
                .execute(&mut connection)
                .await
                .unwrap();
            connection.close().await.unwrap();
            return;
        }
        let paths: Vec<_> = if backend.directory() {
            fs::read_dir(backend.path())
                .unwrap()
                .map(|e| e.unwrap().path())
                .filter(|p| p.file_name().unwrap() != ".tui-journal.lock")
                .collect()
        } else {
            vec![backend.path().to_owned()]
        };
        for path in paths {
            let text = fs::read_to_string(&path).unwrap();
            let changed = text
                .replace("secret title canary", "external title edit")
                .replace("Locked entry", "External edit");
            assert_ne!(changed, text);
            fs::write(path, changed).unwrap();
        }
    }
    #[tokio::test]
    async fn writes_keep_native_conflict_detection_after_explicit_load() {
        for protected in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            for backend in backends(dir.path()) {
                let mut provider = JournalProvider::open(backend.clone()).await.unwrap();
                provider.add_entry(draft()).await.unwrap();
                if protected {
                    provider
                        .enable_journal_protection(password(), None)
                        .await
                        .unwrap();
                }
                let mut entry = provider.load_all_entries().await.unwrap().remove(0);
                external_edit(&backend).await;
                let before = store_hash(backend.path()).unwrap();
                entry.content = "stale app update".into();
                assert!(
                    provider.update_entry(entry).await.is_err(),
                    "missed conflict {backend:?}, protected={protected}"
                );
                assert_eq!(before, store_hash(backend.path()).unwrap());
            }
        }
    }

    #[tokio::test]
    async fn failed_conversion_requires_a_visible_reload_before_writing() {
        let dir = tempfile::tempdir().unwrap();
        for backend in backends(dir.path()) {
            let mut provider = JournalProvider::open(backend.clone()).await.unwrap();
            provider.add_entry(draft()).await.unwrap();
            let mut stale = provider.load_all_entries().await.unwrap().remove(0);
            external_edit(&backend).await;
            match &backend {
                #[cfg(feature = "json")]
                JournalBackend::Json(path) => {
                    let mut value: serde_json::Value =
                        serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
                    let entries = if value.is_array() {
                        value.as_array_mut().unwrap()
                    } else {
                        value["entries"].as_array_mut().unwrap()
                    };
                    entries[0]["foreign_metadata"] = "preserve this".into();
                    fs::write(path, serde_json::to_vec(&value).unwrap()).unwrap();
                }
                #[cfg(feature = "sqlite")]
                JournalBackend::Sqlite(path) => {
                    use sqlx::Connection;
                    let options = sqlx::sqlite::SqliteConnectOptions::new().filename(path);
                    let mut connection = sqlx::SqliteConnection::connect_with(&options)
                        .await
                        .unwrap();
                    sqlx::query("CREATE TABLE unrelated (value TEXT)")
                        .execute(&mut connection)
                        .await
                        .unwrap();
                    connection.close().await.unwrap();
                }
                #[cfg(feature = "file")]
                JournalBackend::File(path, _) => {
                    fs::write(path.join("unrelated.txt"), "preserve this").unwrap()
                }
                #[cfg(feature = "vjournal")]
                JournalBackend::Vjournal(path) => {
                    fs::write(path.join("unrelated.txt"), "preserve this").unwrap()
                }
            }
            let before = store_hash(backend.path()).unwrap();
            let records = provider.journal_records().await.unwrap();
            assert!(
                provider
                    .enable_journal_protection(password(), Some(records))
                    .await
                    .is_err()
            );
            assert!(provider.journal_reload_required());
            stale.content = "stale edit must not overwrite external data".into();
            assert!(provider.update_entry(stale).await.is_err());
            assert_eq!(store_hash(backend.path()).unwrap(), before);
            let fresh = provider.load_all_entries().await.unwrap();
            assert!(!provider.journal_reload_required());
            assert!(fresh[0].title.contains("external"));
        }
    }
    #[cfg(feature = "vjournal")]
    #[test]
    fn calendar_preflight_rejects_orphan_recurrences_and_duplicate_masters() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JournalBackend::Vjournal(dir.path().to_owned());
        let master = "BEGIN:VJOURNAL\r\nUID:one\r\nDTSTART;VALUE=DATE:20240101\r\nSUMMARY:Title\r\nEND:VJOURNAL\r\n";
        let orphan = "BEGIN:VJOURNAL\r\nUID:orphan\r\nRECURRENCE-ID;VALUE=DATE:20240102\r\nDTSTART;VALUE=DATE:20240102\r\nSUMMARY:Orphan\r\nEND:VJOURNAL\r\n";
        fs::write(
            dir.path().join("one.ics"),
            format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n{master}{orphan}END:VCALENDAR\r\n"),
        )
        .unwrap();
        assert!(preflight(&backend).is_err());
        let calendar = format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n{master}END:VCALENDAR\r\n");
        fs::write(dir.path().join("one.ics"), &calendar).unwrap();
        fs::write(dir.path().join("two.ics"), calendar).unwrap();
        assert!(preflight(&backend).is_err());
    }

    #[cfg(feature = "file")]
    #[tokio::test]
    async fn file_conversion_reports_unrelated_filenames_and_preserves_all_files() {
        for name in [".sync.ffs_db", ".tui_journal.lock", "scratch.md"] {
            let dir = tempfile::tempdir().unwrap();
            let backend =
                JournalBackend::File(dir.path().join("entries"), crate::FileFormat::Plain);
            let mut provider = JournalProvider::open(backend.clone()).await.unwrap();
            provider.add_entry(draft()).await.unwrap();
            let unrelated = backend.path().join(name);
            fs::write(
                &unrelated,
                "Unrelated data must survive conversion attempts",
            )
            .unwrap();
            let before = store_hash(backend.path()).unwrap();
            let error = provider
                .enable_journal_protection(password(), None)
                .await
                .unwrap_err();
            let details = format!("{error:#}");
            assert!(
                details.contains(&unrelated.display().to_string()),
                "{details}"
            );
            assert!(
                details.contains("Move non-entry files outside the journal folder"),
                "{details}"
            );
            assert_eq!(store_hash(backend.path()).unwrap(), before);
            assert!(!backend.pending_path().exists());
            assert!(!backend.header_path().exists());
            assert!(provider.journal_reload_required());
        }
    }

    #[cfg(feature = "file")]
    #[tokio::test]
    async fn legacy_file_journal_can_be_protected_after_moving_unrelated_files_aside() {
        for format in [crate::FileFormat::Plain, crate::FileFormat::Markdown] {
            let dir = tempfile::tempdir().unwrap();
            let backend = JournalBackend::File(dir.path().join("entries"), format);
            fs::create_dir(backend.path()).unwrap();
            let name = match format {
                crate::FileFormat::Plain => "20261001_120000",
                crate::FileFormat::Markdown => "20261001_120000.md",
            };
            fs::write(backend.path().join(name), "---\ntitle: Legacy journal entry\ndate: 2026-10-01T02:00:00Z\ncreated: 2026-10-01T12:00:00+10:00\ntags: [legacy]\ntui_journal:\n  version: 1\n  id: 11e02e5c-7cbf-4828-8a46-8c8712461e00\n  priority: 2\nextra_property: retain this value\n---\nOriginal legacy content\n").unwrap();
            let unrelated = backend.path().join(".sync.ffs_db");
            fs::write(&unrelated, "Keep this non-entry file").unwrap();
            let mut provider = JournalProvider::open(backend.clone()).await.unwrap();
            let original = provider.load_all_entries().await.unwrap().remove(0);
            assert!(
                provider
                    .enable_journal_protection(password(), None)
                    .await
                    .is_err()
            );
            drop(provider);
            let moved = dir.path().join("saved-sync-metadata");
            fs::rename(&unrelated, &moved).unwrap();

            let mut provider = JournalProvider::open(backend.clone()).await.unwrap();
            provider
                .enable_journal_protection(password(), None)
                .await
                .unwrap();
            drop(provider);
            let mut provider = JournalProvider::open(backend.clone()).await.unwrap();
            assert!(provider.is_locked());
            provider.unlock(password()).await.unwrap();
            let mut restored = provider.load_all_entries().await.unwrap().remove(0);
            assert!(
                provider
                    .protection_metadata(restored.id)
                    .await
                    .unwrap()
                    .unwrap()
                    .contains("retain this value")
            );
            restored.id = original.id;
            assert_eq!(restored, original);
            assert_eq!(
                fs::read_to_string(&moved).unwrap(),
                "Keep this non-entry file"
            );
        }
    }
    #[tokio::test]
    async fn conversion_refuses_edits_after_records_were_captured() {
        let dir = tempfile::tempdir().unwrap();
        for backend in backends(dir.path()) {
            let mut provider = JournalProvider::open(backend.clone()).await.unwrap();
            provider.add_entry(draft()).await.unwrap();
            let old_hash = store_hash(backend.path()).unwrap();
            let records = provider.journal_records().await.unwrap();
            external_edit(&backend).await;
            let after = store_hash(backend.path()).unwrap();
            let key = age::x25519::Identity::generate();
            let header = Header {
                format: HEADER_FORMAT.into(),
                version: 1,
                identity: key.to_public().to_string(),
                wrapped_key: String::new(),
            };
            assert!(
                provider
                    .convert(records, Some(header), Some(key), old_hash)
                    .await
                    .is_err()
            );
            assert_eq!(after, store_hash(backend.path()).unwrap());
            assert!(!backend.pending_path().exists());
            assert!(!backend.header_path().exists());
        }
    }
    #[test]
    fn migration_hashes_include_file_and_directory_boundaries() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        fs::create_dir(&first).unwrap();
        fs::create_dir(&second).unwrap();
        fs::create_dir(first.join("a")).unwrap();
        fs::create_dir(second.join("a")).unwrap();
        fs::write(first.join("a").join("b"), "secret").unwrap();
        fs::write(second.join("b"), "secret").unwrap();
        assert_ne!(store_hash(&first).unwrap(), store_hash(&second).unwrap());
    }
    #[test]
    fn publication_recovers_after_the_original_was_renamed() {
        let dir = tempfile::tempdir().unwrap();
        let backend = backends(dir.path()).remove(0);
        fs::write(backend.path(), "original").unwrap();
        let token = random_token();
        let stage = sibling(backend.path(), &format!(".myna-stage-{token}"));
        let previous = sibling(backend.path(), &format!(".myna-previous-{token}"));
        fs::write(&stage, "encrypted replacement").unwrap();
        let key = age::x25519::Identity::generate();
        let header = Header {
            format: HEADER_FORMAT.into(),
            version: 1,
            identity: key.to_public().to_string(),
            wrapped_key: String::new(),
        };
        let tx = Transition {
            version: 1,
            token,
            old_hash: store_hash(backend.path()).unwrap(),
            new_hash: store_hash(&stage).unwrap(),
            header: Some(header),
        };
        write_json(&backend.pending_path(), &tx).unwrap();
        fs::rename(backend.path(), &previous).unwrap();
        recover(&backend).unwrap();
        assert_eq!(
            fs::read_to_string(backend.path()).unwrap(),
            "encrypted replacement"
        );
        assert!(backend.header_path().exists());
        assert!(!previous.exists());
        assert!(!stage.exists());
        assert!(!backend.pending_path().exists());
    }
    #[test]
    fn publication_recovery_removes_owned_native_temporaries_at_every_phase() {
        for phase in 0..3 {
            let dir = tempfile::tempdir().unwrap();
            let backend = backends(dir.path()).remove(0);
            fs::write(backend.path(), "original").unwrap();
            let token = random_token();
            let work = create_transaction_directory(&backend, &token).unwrap();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                assert_eq!(
                    fs::metadata(&work).unwrap().permissions().mode() & 0o777,
                    0o700
                );
            }
            let stage = work.join("store");
            let previous = sibling(backend.path(), &format!(".myna-previous-{token}"));
            fs::write(&stage, "replacement").unwrap();
            for filename in ["store-journal", "store-wal", "store-shm", ".tmp-json-write"] {
                fs::write(work.join(filename), "private native temporary").unwrap();
            }
            let tx = Transition {
                version: 2,
                token,
                old_hash: store_hash(backend.path()).unwrap(),
                new_hash: store_hash(&stage).unwrap(),
                header: None,
            };
            write_json(&backend.pending_path(), &tx).unwrap();
            if phase >= 1 {
                fs::rename(backend.path(), &previous).unwrap();
            }
            if phase == 2 {
                fs::rename(&stage, backend.path()).unwrap();
            }
            recover(&backend).unwrap();
            assert_eq!(fs::read_to_string(backend.path()).unwrap(), "replacement");
            assert!(!work.exists());
            assert!(!previous.exists());
            assert!(!backend.pending_path().exists());
        }
    }
    #[cfg(feature = "vjournal")]
    #[test]
    fn protected_calendar_entries_keep_native_priority_rules() {
        let backend = JournalBackend::Vjournal(PathBuf::from("unused"));
        let mut entry = Entry::from_draft(0, draft());
        entry.priority = Some(0);
        normalize_native_entry(&backend, &mut entry).unwrap();
        assert_eq!(entry.priority, None);
        entry.priority = Some(10);
        assert!(normalize_native_entry(&backend, &mut entry).is_err());
        entry.priority = None;
        entry.title.clear();
        assert!(normalize_native_entry(&backend, &mut entry).is_err());
    }
    #[cfg(feature = "json")]
    #[test]
    fn conversion_rejects_unknown_json_data_without_dropping_it() {
        let dir = tempfile::tempdir().unwrap();
        let backend = JournalBackend::Json(dir.path().join("entries.json"));
        let mut value = serde_json::to_value(vec![Entry::from_draft(0, draft())]).unwrap();
        value[0]["custom_metadata"] = serde_json::json!("private canary");
        fs::write(backend.path(), serde_json::to_vec(&value).unwrap()).unwrap();
        assert!(preflight(&backend).is_err());
        assert!(
            fs::read_to_string(backend.path())
                .unwrap()
                .contains("private canary")
        );
    }
    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn conversion_rejects_custom_sqlite_names_and_generated_columns() {
        use sqlx::Connection;
        for schema in [
            "CREATE TABLE sqliteNotes (value TEXT)",
            "ALTER TABLE entries ADD COLUMN custom TEXT GENERATED ALWAYS AS (title) VIRTUAL",
            "ALTER TABLE tags ADD COLUMN custom TEXT GENERATED ALWAYS AS (tag) VIRTUAL",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("entries.db");
            let backend = JournalBackend::Sqlite(path.clone());
            let mut provider = JournalProvider::open(backend.clone()).await.unwrap();
            provider.add_entry(draft()).await.unwrap();
            let options = sqlx::sqlite::SqliteConnectOptions::new().filename(path);
            let mut connection = sqlx::SqliteConnection::connect_with(&options)
                .await
                .unwrap();
            sqlx::query(schema).execute(&mut connection).await.unwrap();
            connection.close().await.unwrap();
            let before = store_hash(backend.path()).unwrap();
            assert!(
                provider
                    .enable_journal_protection(password(), None)
                    .await
                    .is_err()
            );
            assert_eq!(store_hash(backend.path()).unwrap(), before);
            assert!(!backend.pending_path().exists());
            assert!(!backend.header_path().exists());
        }
    }
    #[test]
    fn native_lock_name_cannot_hide_unrelated_data() {
        let dir = tempfile::tempdir().unwrap();
        let lock = dir.path().join(".tui-journal.lock");
        fs::create_dir(&lock).unwrap();
        fs::write(lock.join("keep"), "private data").unwrap();
        assert!(store_hash(dir.path()).is_err());
        assert!(lock.join("keep").exists());
    }
    #[test]
    fn ciphertext_created_with_only_the_public_recipient_is_rejected() {
        let key = age::x25519::Identity::generate();
        let header = Header {
            format: HEADER_FORMAT.into(),
            version: 1,
            identity: key.to_public().to_string(),
            wrapped_key: String::new(),
        };
        let identity = age::x25519::Identity::generate().to_public().to_string();
        let forged = Payload {
            format: RECORD_FORMAT.into(),
            journal: header.identity.clone(),
            identity: identity.clone(),
            record: JournalRecord {
                entry: Entry::from_draft(0, draft()),
                native_metadata: None,
            },
            authentication: [0; 32],
        };
        let recipient = header.identity.parse::<age::x25519::Recipient>().unwrap();
        let ciphertext =
            age::encrypt_and_armor(&recipient, &serde_json::to_vec(&forged).unwrap()).unwrap();
        let envelope = EncryptedNote {
            version: RECORD_VERSION,
            identity,
            ciphertext,
        };
        let entry = Entry::new(
            0,
            DateTime::UNIX_EPOCH,
            "Locked entry".into(),
            String::new(),
            Vec::new(),
            None,
        )
        .with_protection(envelope);
        let error = decrypt_record(&header, &key, entry).unwrap_err();
        assert!(error.to_string().contains("authentication"));
    }
    #[tokio::test]
    async fn plain_stores_reject_raw_journal_ciphertext_before_any_write() {
        let dir = tempfile::tempdir().unwrap();
        let key = age::x25519::Identity::generate();
        let header = Header {
            format: HEADER_FORMAT.into(),
            version: 1,
            identity: key.to_public().to_string(),
            wrapped_key: String::new(),
        };
        let encrypted = encrypt_record(
            &header,
            &key,
            &JournalRecord {
                entry: Entry::from_draft(0, draft()),
                native_metadata: None,
            },
        )
        .unwrap();
        for backend in backends(dir.path()) {
            let mut p = JournalProvider::open(backend.clone()).await.unwrap();
            let original = p.add_entry(draft()).await.unwrap();
            let before = store_hash(backend.path()).unwrap();
            let mut candidate = encrypted.clone();
            candidate.id = original.id;
            assert!(
                p.add_entry(EntryDraft::from_entry(candidate.clone()))
                    .await
                    .is_err()
            );
            assert!(p.update_entry(candidate.clone()).await.is_err());
            assert!(p.restore_entry(candidate.clone()).await.is_err());
            assert!(
                p.import_entries(EntriesDTO::new(vec![
                    draft(),
                    EntryDraft::from_entry(candidate)
                ]))
                .await
                .is_err()
            );
            assert_eq!(before, store_hash(backend.path()).unwrap());
        }
    }
    #[tokio::test]
    async fn empty_journals_can_be_protected_and_unprotected() {
        let dir = tempfile::tempdir().unwrap();
        for backend in backends(dir.path()) {
            let mut p = JournalProvider::open(backend).await.unwrap();
            p.enable_journal_protection(password(), None).await.unwrap();
            assert!(p.load_all_entries().await.unwrap().is_empty());
            p.disable_journal_protection(password()).await.unwrap();
        }
    }
    #[tokio::test]
    async fn missing_headers_fail_closed_before_native_writes() {
        let dir = tempfile::tempdir().unwrap();
        for backend in backends(dir.path()) {
            let mut p = JournalProvider::open(backend.clone()).await.unwrap();
            p.add_entry(draft()).await.unwrap();
            p.enable_journal_protection(password(), None).await.unwrap();
            drop(p);
            fs::remove_file(backend.header_path()).unwrap();
            let before = store_hash(backend.path()).unwrap();
            let mut p = JournalProvider::open(backend.clone()).await.unwrap();
            assert!(p.load_all_entries().await.is_err());
            assert!(p.add_entry(draft()).await.is_err());
            assert_eq!(before, store_hash(backend.path()).unwrap());
        }
    }
    #[tokio::test]
    async fn interrupted_plaintext_staging_and_native_temporaries_are_removed_before_opening() {
        let dir = tempfile::tempdir().unwrap();
        for backend in backends(dir.path()) {
            let mut p = JournalProvider::open(backend.clone()).await.unwrap();
            p.add_entry(draft()).await.unwrap();
            p.enable_journal_protection(password(), None).await.unwrap();
            drop(p);
            let original_header = fs::read(backend.header_path()).unwrap();
            let tx = Transition {
                version: 2,
                token: random_token(),
                old_hash: store_hash(backend.path()).unwrap(),
                new_hash: None,
                header: None,
            };
            let work = create_transaction_directory(&backend, &tx.token).unwrap();
            write_json(&backend.pending_path(), &tx).unwrap();
            for filename in [
                "store",
                "store-journal",
                "store-wal",
                "store-shm",
                ".tmp-json-write",
            ] {
                fs::write(work.join(filename), "plaintext interrupted conversion").unwrap();
            }
            let unrelated = sibling(backend.path(), ".unrelated-user-file");
            fs::write(&unrelated, "retain unrelated data").unwrap();
            assert!(JournalLease::acquire(&backend).is_err());
            assert_eq!(
                JournalProvider::status(&backend).unwrap(),
                JournalStatus::Protected
            );
            let p = JournalProvider::open(backend.clone()).await.unwrap();
            assert!(p.is_locked());
            assert!(!work.exists());
            assert!(!backend.pending_path().exists());
            assert_eq!(fs::read(backend.header_path()).unwrap(), original_header);
            assert_eq!(
                fs::read_to_string(unrelated).unwrap(),
                "retain unrelated data"
            );
        }
    }
    #[test]
    fn recovery_refuses_to_remove_a_directory_without_its_ownership_marker() {
        let dir = tempfile::tempdir().unwrap();
        let backend = backends(dir.path()).remove(0);
        fs::write(backend.path(), "original").unwrap();
        let tx = Transition {
            version: 2,
            token: random_token(),
            old_hash: store_hash(backend.path()).unwrap(),
            new_hash: None,
            header: None,
        };
        let work = transaction_directory(&backend, &tx.token);
        fs::create_dir(&work).unwrap();
        fs::write(work.join("keep"), "unrelated data").unwrap();
        write_json(&backend.pending_path(), &tx).unwrap();
        assert!(recover(&backend).is_err());
        assert_eq!(
            fs::read_to_string(work.join("keep")).unwrap(),
            "unrelated data"
        );
        assert!(backend.pending_path().exists());
    }
    #[test]
    fn legacy_unfinished_staging_requires_manual_recovery() {
        let dir = tempfile::tempdir().unwrap();
        let backend = backends(dir.path()).remove(0);
        fs::write(backend.path(), "original").unwrap();
        let tx = Transition {
            version: 1,
            token: random_token(),
            old_hash: store_hash(backend.path()).unwrap(),
            new_hash: None,
            header: None,
        };
        write_json(&backend.pending_path(), &tx).unwrap();
        let error = recover(&backend).unwrap_err();
        assert!(error.to_string().contains("older preview"));
        assert!(backend.pending_path().exists());
    }
    #[tokio::test]
    async fn password_rotation_keeps_ciphertext_and_rejects_the_previous_password() {
        let dir = tempfile::tempdir().unwrap();
        let backend = backends(dir.path()).remove(0);
        let mut p = JournalProvider::open(backend.clone()).await.unwrap();
        p.add_entry(draft()).await.unwrap();
        p.enable_journal_protection(password(), None).await.unwrap();
        let before = store_hash(backend.path()).unwrap();
        p.change_journal_password(password(), SecretString::from("new password".to_string()))
            .await
            .unwrap();
        assert_eq!(before, store_hash(backend.path()).unwrap());
        drop(p);
        let mut p = JournalProvider::open(backend).await.unwrap();
        assert!(p.unlock(password()).await.is_err());
        p.unlock(SecretString::from("new password".to_string()))
            .await
            .unwrap();
        assert_eq!(p.load_all_entries().await.unwrap().len(), 1);
    }
    #[test]
    fn journal_keys_reject_non_passphrase_envelopes() {
        let key = age::x25519::Identity::generate();
        let header = Header {
            format: HEADER_FORMAT.into(),
            version: 1,
            identity: key.to_public().to_string(),
            wrapped_key: age::encrypt_and_armor(
                &key.to_public(),
                key.to_string().expose_secret().as_bytes(),
            )
            .unwrap(),
        };
        let error = unwrap_key(&header, &password()).err().unwrap();
        assert_eq!(error.to_string(), "Invalid journal key protection");
    }

    #[test]
    fn journal_records_reject_passphrase_and_non_x25519_envelopes() {
        let key = age::x25519::Identity::generate();
        let header = Header {
            format: HEADER_FORMAT.into(),
            version: 1,
            identity: key.to_public().to_string(),
            wrapped_key: String::new(),
        };
        let record = JournalRecord {
            entry: Entry::from_draft(7, draft()),
            native_metadata: None,
        };
        let encrypted = encrypt_record(&header, &key, &record).unwrap();
        let mut passphrase_entry = encrypted.clone();
        let mut recipient = age::scrypt::Recipient::new(password());
        recipient.set_work_factor(1);
        passphrase_entry.protection.as_mut().unwrap().ciphertext =
            age::encrypt_and_armor(&recipient, b"{}").unwrap();
        assert_eq!(
            decrypt_record(&header, &key, passphrase_entry)
                .unwrap_err()
                .to_string(),
            "Invalid journal entry key type"
        );

        let mut other_recipient_entry = encrypted;
        let envelope = other_recipient_entry.protection.as_mut().unwrap();
        let mut binary = Vec::new();
        age::armor::ArmoredReader::new(envelope.ciphertext.as_bytes())
            .read_to_end(&mut binary)
            .unwrap();
        let offset = binary
            .windows(b"-> X25519 ".len())
            .position(|window| window == b"-> X25519 ")
            .unwrap();
        binary.splice(offset..offset + b"-> X25519 ".len(), *b"-> p256tag ");
        let mut armored = Vec::new();
        let mut writer =
            age::armor::ArmoredWriter::wrap_output(&mut armored, age::armor::Format::AsciiArmor)
                .unwrap();
        writer.write_all(&binary).unwrap();
        writer.finish().unwrap();
        envelope.ciphertext = String::from_utf8(armored).unwrap();
        assert_eq!(
            decrypt_record(&header, &key, other_recipient_entry)
                .unwrap_err()
                .to_string(),
            "Journal entry is damaged or belongs to another journal"
        );
    }

    #[tokio::test]
    async fn journal_records_reject_tampering_and_cross_journal_keys() {
        let key = age::x25519::Identity::generate();
        let header = Header {
            format: HEADER_FORMAT.into(),
            version: 1,
            identity: key.to_public().to_string(),
            wrapped_key: String::new(),
        };
        let record = JournalRecord {
            entry: Entry::from_draft(7, draft()),
            native_metadata: Some("private native metadata".into()),
        };
        let encrypted = encrypt_record(&header, &key, &record).unwrap();
        let mut changed = encrypted.clone();
        changed.title = "forged".into();
        assert!(decrypt_record(&header, &key, changed).is_err());
        let mut changed = encrypted.clone();
        changed.protection.as_mut().unwrap().identity =
            age::x25519::Identity::generate().to_public().to_string();
        assert!(decrypt_record(&header, &key, changed).is_err());
        assert!(decrypt_record(&header, &age::x25519::Identity::generate(), encrypted).is_err());
    }
}

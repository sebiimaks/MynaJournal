//! Local journal files with portable YAML front matter.

mod document;

use std::{
    collections::{HashMap, HashSet},
    fs::{self, File, Metadata, OpenOptions, Permissions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use chrono::Local;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use self::document::JournalDocument;
use super::{DataProvider, EntriesDTO, Entry, EntryDraft, ModifyEntryError};

/// The extension used for newly created journal files.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FileFormat {
    #[default]
    Plain,
    Markdown,
}

impl FileFormat {
    fn extension(self) -> &'static str {
        match self {
            Self::Plain => "",
            Self::Markdown => ".md",
        }
    }
}

/// Persists each entry in a separate extensionless or Markdown file.
pub struct FileDataProvider {
    directory: PathBuf,
    format: FileFormat,
    guard: Option<DirectoryGuard>,
    initialized: bool,
    next_id: u64,
    uid_to_id: HashMap<Uuid, u32>,
    entries: HashMap<u32, StoredEntry>,
    deleted: HashMap<u32, RestoreMetadata>,
}

#[derive(Clone)]
struct StoredEntry {
    document: JournalDocument,
    path: PathBuf,
    baseline: Snapshot,
}

struct RestoreMetadata {
    document: JournalDocument,
    path: PathBuf,
    permissions: Permissions,
}

#[derive(Clone, PartialEq, Eq)]
struct Snapshot {
    bytes: Vec<u8>,
    identity: FileIdentity,
}

#[derive(Clone, PartialEq, Eq)]
struct FileIdentity {
    object: ObjectIdentity,
    length: u64,
    modified: Option<SystemTime>,
    permissions: Permissions,
}

#[derive(Clone, PartialEq, Eq)]
struct ObjectIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(not(unix))]
    created: Option<SystemTime>,
}

struct DirectoryGuard {
    requested: PathBuf,
    canonical: PathBuf,
    identity: ObjectIdentity,
    lock: File,
    lock_identity: ObjectIdentity,
}

impl FileDataProvider {
    /// Opens the directory lazily. The first operation obtains an exclusive app lock.
    pub fn new(directory: PathBuf, format: FileFormat) -> Self {
        Self {
            directory,
            format,
            guard: None,
            initialized: false,
            next_id: 0,
            uid_to_id: HashMap::new(),
            entries: HashMap::new(),
            deleted: HashMap::new(),
        }
    }

    fn guard(&mut self) -> Result<&DirectoryGuard> {
        if self.guard.is_none() {
            self.guard = Some(DirectoryGuard::acquire(&self.directory)?);
        }
        let guard = self.guard.as_ref().expect("directory guard was acquired");
        guard.check()?;
        Ok(guard)
    }

    fn scan(&mut self) -> Result<Vec<StoredEntry>> {
        let guard = self.guard()?;
        let mut paths = fs::read_dir(&guard.canonical)
            .with_context(|| format!("Reading journal directory {}", guard.canonical.display()))?
            .map(|item| item.map(|item| item.path()))
            .collect::<std::io::Result<Vec<_>>>()?;
        paths.sort();
        let mut discovered = Vec::new();
        let mut seen = HashMap::new();
        for path in paths {
            if !is_journal_path(&path) {
                continue;
            }
            let metadata = fs::symlink_metadata(&path)
                .with_context(|| format!("Inspecting journal file {}", path.display()))?;
            ensure!(
                !metadata.file_type().is_symlink(),
                "Journal file symlinks are not supported: {}",
                path.display()
            );
            if !metadata.is_file() {
                continue;
            }
            let baseline = Snapshot::read(&path)?;
            let text = match std::str::from_utf8(&baseline.bytes) {
                Ok(text) => text,
                Err(error) => {
                    // Use the same recognition rules for BOMs and quoted YAML keys as the parser.
                    if JournalDocument::parse(&String::from_utf8_lossy(&baseline.bytes))
                        .with_context(|| format!("Invalid journal file {}", path.display()))?
                        .is_some()
                    {
                        return Err(error)
                            .with_context(|| format!("Journal is not UTF-8: {}", path.display()));
                    }
                    continue;
                }
            };
            let Some(document) = JournalDocument::parse(text)
                .with_context(|| format!("Invalid journal file {}", path.display()))?
            else {
                continue;
            };
            if let Some(previous) = seen.insert(document.uid, path.clone()) {
                bail!(
                    "Duplicate journal UUID {} in {} and {}; resolve it before writing",
                    document.uid,
                    previous.display(),
                    path.display()
                );
            }
            discovered.push(StoredEntry {
                document,
                path,
                baseline,
            });
        }
        guard.check()?;
        Ok(discovered)
    }

    fn reload(&mut self) -> Result<Vec<Entry>> {
        let discovered = self.scan()?;
        let new_ids = discovered
            .iter()
            .filter(|stored| !self.uid_to_id.contains_key(&stored.document.uid))
            .count() as u64;
        ensure!(
            self.next_id + new_ids <= u64::from(u32::MAX) + 1,
            "Journal session has exhausted its available entry IDs; restart the app"
        );
        let mut entries = HashMap::new();
        let mut loaded = Vec::new();
        for stored in discovered {
            let uid = stored.document.uid;
            let id = if let Some(id) = self.uid_to_id.get(&uid) {
                *id
            } else {
                let id = self.next_id as u32;
                self.next_id += 1;
                self.uid_to_id.insert(uid, id);
                id
            };
            loaded.push(stored.document.entry(id)?);
            entries.insert(id, stored);
        }
        self.entries = entries;
        self.initialized = true;
        Ok(loaded)
    }

    fn ensure_initialized(&mut self) -> Result<()> {
        if !self.initialized {
            self.reload()?;
        }
        Ok(())
    }

    fn prepare_write(&mut self) -> Result<Vec<StoredEntry>> {
        self.ensure_initialized()?;
        // Validate the directory without accepting external edits as new baselines.
        self.scan()
    }

    fn next_id(&self) -> Result<u32> {
        u32::try_from(self.next_id)
            .context("Journal session has exhausted its available entry IDs; restart the app")
    }

    fn new_path(&self, document: &JournalDocument) -> Result<PathBuf> {
        let root = &self
            .guard
            .as_ref()
            .context("Journal directory is not open")?
            .canonical;
        let stem = document.created.format("%Y%m%d_%H%M%S");
        for suffix in 1_u32..=u32::MAX {
            let name = if suffix == 1 {
                format!("{stem}{}", self.format.extension())
            } else {
                format!("{stem}_{suffix}{}", self.format.extension())
            };
            let path = root.join(name);
            match fs::symlink_metadata(&path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(path),
                Err(error) => return Err(error).context("Checking journal filename availability"),
                Ok(_) => {}
            }
        }
        bail!("No unused journal filename is available")
    }

    fn write_file(
        &self,
        path: &Path,
        document: &JournalDocument,
        baseline: Option<&Snapshot>,
        restored_permissions: Option<&Permissions>,
    ) -> Result<Snapshot> {
        let guard = self
            .guard
            .as_ref()
            .context("Journal directory is not open")?;
        guard.check()?;
        check_destination(path, baseline)?;
        let text = document.text()?;
        let mut temporary = tempfile::Builder::new()
            .prefix(".tui-journal-")
            .suffix(".tmp")
            .tempfile_in(&guard.canonical)
            .context("Creating temporary journal file")?;
        temporary
            .write_all(text.as_bytes())
            .context("Writing temporary journal file")?;
        if let Some(permissions) = baseline
            .map(|baseline| &baseline.identity.permissions)
            .or(restored_permissions)
        {
            temporary
                .as_file()
                .set_permissions(permissions.clone())
                .context("Preserving journal permissions")?;
        }
        temporary
            .as_file()
            .sync_all()
            .context("Flushing journal file")?;
        let identity = FileIdentity::new(&temporary.as_file().metadata()?);
        guard.check()?;
        check_destination(path, baseline)?;
        if baseline.is_some() {
            temporary.persist(path)
        } else {
            temporary.persist_noclobber(path)
        }
        .with_context(|| format!("Saving journal file {}", path.display()))?;
        guard.sync_directory();
        Ok(Snapshot {
            bytes: text.into_bytes(),
            identity,
        })
    }

    fn insert_stored(&mut self, id: u32, stored: StoredEntry) {
        self.uid_to_id.insert(stored.document.uid, id);
        self.next_id = self.next_id.max(u64::from(id) + 1);
        self.entries.insert(id, stored);
        self.deleted.remove(&id);
    }
}

impl DataProvider for FileDataProvider {
    async fn load_all_entries(&mut self) -> Result<Vec<Entry>> {
        self.reload()
    }

    async fn add_entry(&mut self, entry: EntryDraft) -> Result<Entry, ModifyEntryError> {
        validate_title(&entry.title)?;
        self.prepare_write()?;
        let id = self.next_id()?;
        let document = JournalDocument::new(&entry, Local::now().fixed_offset())?;
        let path = self.new_path(&document)?;
        let baseline = self.write_file(&path, &document, None, None)?;
        let entry = document.entry(id)?;
        self.insert_stored(
            id,
            StoredEntry {
                document,
                path,
                baseline,
            },
        );
        Ok(entry)
    }

    async fn restore_entry(&mut self, entry: Entry) -> Result<Entry, ModifyEntryError> {
        validate_title(&entry.title)?;
        let discovered = self.prepare_write()?;
        if self.entries.contains_key(&entry.id) {
            return Err(anyhow!("Entry id {} already exists", entry.id).into());
        }
        let (document, path) = if let Some(metadata) = self.deleted.get(&entry.id) {
            (metadata.document.with_entry(&entry)?, metadata.path.clone())
        } else {
            if self.uid_to_id.values().any(|id| *id == entry.id) {
                return Err(anyhow!(
                    "Restore metadata for entry {} is no longer available",
                    entry.id
                )
                .into());
            }
            let document = JournalDocument::new(
                &EntryDraft::from_entry(entry.clone()),
                Local::now().fixed_offset(),
            )?;
            let path = self.new_path(&document)?;
            (document, path)
        };
        if discovered
            .iter()
            .any(|stored| stored.document.uid == document.uid)
        {
            return Err(anyhow!(
                "Journal UUID {} already exists; reload before restoring this entry",
                document.uid
            )
            .into());
        }
        let permissions = self
            .deleted
            .get(&entry.id)
            .map(|metadata| &metadata.permissions);
        let baseline = self.write_file(&path, &document, None, permissions)?;
        let entry = document.entry(entry.id)?;
        self.insert_stored(
            entry.id,
            StoredEntry {
                document,
                path,
                baseline,
            },
        );
        Ok(entry)
    }

    async fn remove_entry(&mut self, entry_id: u32) -> Result<()> {
        self.prepare_write()?;
        let stored = self
            .entries
            .get(&entry_id)
            .ok_or_else(|| anyhow!("No entry with id {entry_id}"))?;
        self.guard
            .as_ref()
            .context("Journal directory is not open")?
            .check()?;
        check_destination(&stored.path, Some(&stored.baseline))?;
        fs::remove_file(&stored.path)
            .with_context(|| format!("Deleting journal file {}", stored.path.display()))?;
        self.guard
            .as_ref()
            .expect("directory is open")
            .sync_directory();
        let stored = self.entries.remove(&entry_id).expect("entry was checked");
        self.deleted.insert(
            entry_id,
            RestoreMetadata {
                document: stored.document,
                path: stored.path,
                permissions: stored.baseline.identity.permissions,
            },
        );
        Ok(())
    }

    async fn update_entry(&mut self, entry: Entry) -> Result<Entry, ModifyEntryError> {
        validate_title(&entry.title)?;
        self.prepare_write()?;
        let stored = self
            .entries
            .get(&entry.id)
            .ok_or_else(|| anyhow!("No entry with id {}", entry.id))?;
        let document = stored.document.with_entry(&entry)?;
        let path = stored.path.clone();
        let baseline = self.write_file(&path, &document, Some(&stored.baseline), None)?;
        let entry = document.entry(entry.id)?;
        self.insert_stored(
            entry.id,
            StoredEntry {
                document,
                path,
                baseline,
            },
        );
        Ok(entry)
    }

    async fn get_export_object(&mut self, entries_ids: &[u32]) -> Result<EntriesDTO> {
        self.ensure_initialized()?;
        self.guard()?.check()?;
        let entries = entries_ids
            .iter()
            .map(|id| {
                self.entries
                    .get(id)
                    .ok_or_else(|| anyhow!("No entry with id {id}"))?
                    .document
                    .entry(*id)
                    .map(EntryDraft::from_entry)
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(EntriesDTO::new(entries))
    }

    async fn assign_priority_to_entries(&mut self, priority: u32) -> Result<()> {
        self.prepare_write()?;
        let mut entries = self
            .entries
            .iter()
            .map(|(id, stored)| stored.document.entry(*id))
            .collect::<Result<Vec<_>>>()?;
        entries.sort_by_key(|entry| entry.id);
        for mut entry in entries {
            if entry.priority.is_none() {
                entry.priority = Some(priority);
                self.update_entry(entry).await?;
            }
        }
        Ok(())
    }

    fn retain_restore_metadata(&mut self, ids: &[u32]) {
        let ids: HashSet<_> = ids.iter().copied().collect();
        self.deleted.retain(|id, _| ids.contains(id));
    }
}

impl DirectoryGuard {
    fn acquire(directory: &Path) -> Result<Self> {
        let requested = std::path::absolute(directory).context("Resolving journal directory")?;
        fs::create_dir_all(&requested)
            .with_context(|| format!("Creating journal directory {}", requested.display()))?;
        let canonical = fs::canonicalize(&requested).context("Resolving journal directory")?;
        let metadata = fs::metadata(&canonical)?;
        ensure!(
            metadata.is_dir(),
            "Journal path is not a directory: {}",
            requested.display()
        );
        let identity = ObjectIdentity::new(&metadata);
        let lock_path = canonical.join(".tui-journal.lock");
        match fs::symlink_metadata(&lock_path) {
            Ok(metadata) => ensure!(
                metadata.is_file(),
                "Journal lock must be a regular file: {}",
                lock_path.display()
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("Inspecting journal directory lock"),
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options
            .open(&lock_path)
            .with_context(|| format!("Opening journal lock {}", lock_path.display()))?;
        lock.try_lock().map_err(|error| {
            anyhow!(
                "Cannot lock journal directory {}; close any other app using it: {error}",
                requested.display()
            )
        })?;
        let lock_identity = ObjectIdentity::new(&lock.metadata()?);
        let guard = Self {
            requested,
            canonical,
            identity,
            lock,
            lock_identity,
        };
        guard.check()?;
        Ok(guard)
    }

    fn check(&self) -> Result<()> {
        let canonical = fs::canonicalize(&self.requested)
            .context("Journal directory changed or became inaccessible; restart the app")?;
        let metadata = fs::metadata(&canonical)?;
        ensure!(
            canonical == self.canonical
                && metadata.is_dir()
                && ObjectIdentity::new(&metadata) == self.identity,
            "Journal directory was replaced; restart the app before writing"
        );
        let lock_path = self.canonical.join(".tui-journal.lock");
        let metadata = fs::symlink_metadata(&lock_path)
            .context("Journal directory lock disappeared; restart the app before writing")?;
        ensure!(
            metadata.is_file()
                && ObjectIdentity::new(&metadata) == self.lock_identity
                && ObjectIdentity::new(&self.lock.metadata()?) == self.lock_identity,
            "Journal directory lock was replaced; restart the app before writing"
        );
        Ok(())
    }

    fn sync_directory(&self) {
        #[cfg(unix)]
        if let Err(error) = File::open(&self.canonical).and_then(|directory| directory.sync_all()) {
            // The rename/removal already committed; do not report a failed edit after success.
            log::warn!(
                "Could not flush journal directory {}: {error}",
                self.canonical.display()
            );
        }
    }
}

impl ObjectIdentity {
    fn new(metadata: &Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        Self {
            #[cfg(unix)]
            device: metadata.dev(),
            #[cfg(unix)]
            inode: metadata.ino(),
            #[cfg(not(unix))]
            created: metadata.created().ok(),
        }
    }
}

impl FileIdentity {
    fn new(metadata: &Metadata) -> Self {
        Self {
            object: ObjectIdentity::new(metadata),
            length: metadata.len(),
            modified: metadata.modified().ok(),
            permissions: metadata.permissions(),
        }
    }
}

impl Snapshot {
    fn read(path: &Path) -> Result<Self> {
        let metadata = fs::symlink_metadata(path)
            .with_context(|| format!("Inspecting journal file {}", path.display()))?;
        ensure!(
            metadata.is_file(),
            "Journal is not a regular file: {}",
            path.display()
        );
        let identity = FileIdentity::new(&metadata);
        let mut file =
            File::open(path).with_context(|| format!("Opening journal file {}", path.display()))?;
        ensure!(
            identity == FileIdentity::new(&file.metadata()?),
            "Journal changed while opening: {}",
            path.display()
        );
        let mut bytes = Vec::new();
        file.read_to_end(&mut bytes)
            .with_context(|| format!("Reading journal file {}", path.display()))?;
        let current = fs::symlink_metadata(path)?;
        ensure!(
            current.is_file()
                && identity == FileIdentity::new(&current)
                && identity == FileIdentity::new(&file.metadata()?),
            "Journal changed while reading: {}",
            path.display()
        );
        Ok(Self { bytes, identity })
    }
}

fn check_destination(path: &Path, baseline: Option<&Snapshot>) -> Result<()> {
    if let Some(baseline) = baseline {
        let current = Snapshot::read(path).with_context(|| {
            format!(
                "Journal changed or became inaccessible; reload before writing: {}",
                path.display()
            )
        })?;
        ensure!(
            &current == baseline,
            "Journal changed outside the app; reload before writing: {}",
            path.display()
        );
    } else {
        match fs::symlink_metadata(path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).context("Checking journal destination"),
            Ok(_) => bail!(
                "Journal destination is already occupied: {}",
                path.display()
            ),
        }
    }
    Ok(())
}

fn is_journal_path(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| !name.to_string_lossy().starts_with('.'))
        && path.extension().is_none_or(|extension| extension == "md")
}

fn validate_title(title: &str) -> Result<(), ModifyEntryError> {
    if title.is_empty() {
        Err(ModifyEntryError::ValidationError(
            "Entry title can't be empty".into(),
        ))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use chrono::DateTime;

    use super::*;

    #[test]
    fn filenames_use_local_creation_time_and_skip_existing_collision_suffixes() {
        let directory = tempfile::tempdir().unwrap();
        let mut provider = FileDataProvider::new(directory.path().to_owned(), FileFormat::Plain);
        provider.guard().unwrap();
        let created = DateTime::parse_from_rfc3339("2026-10-23T19:42:18+10:00").unwrap();
        let document = JournalDocument::new(
            &EntryDraft::new(created.to_utc(), "Title".into(), vec![], None),
            created,
        )
        .unwrap();
        assert_eq!(
            provider.new_path(&document).unwrap().file_name().unwrap(),
            "20261023_194218"
        );
        for name in ["20261023_194218", "20261023_194218_2"] {
            fs::write(directory.path().join(name), "Already occupied").unwrap();
        }
        let third = provider.new_path(&document).unwrap();
        assert_eq!(third.file_name().unwrap(), "20261023_194218_3");
        provider.write_file(&third, &document, None, None).unwrap();
        assert!(provider.write_file(&third, &document, None, None).is_err());
        assert_eq!(
            fs::read_to_string(directory.path().join("20261023_194218")).unwrap(),
            "Already occupied"
        );
        provider.format = FileFormat::Markdown;
        assert_eq!(
            provider.new_path(&document).unwrap().file_name().unwrap(),
            "20261023_194218.md"
        );
        fs::create_dir(directory.path().join("20261023_194218.md")).unwrap();
        assert_eq!(
            provider.new_path(&document).unwrap().file_name().unwrap(),
            "20261023_194218_2.md"
        );
    }
}

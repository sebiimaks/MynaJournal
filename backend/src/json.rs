use std::{
    fs::{self, File, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
};

use anyhow::{Context, ensure};
use zeroize::Zeroize;

use super::*;

const PROTECTED_STORE_VERSION: u32 = 2;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionedStore {
    format: String,
    version: u32,
    entries: Vec<Entry>,
}

pub struct JsonDataProvide {
    file_path: PathBuf,
    lock: Option<File>,
    #[cfg(unix)]
    directory: Option<File>,
    snapshot: Option<Option<Vec<u8>>>,
    versioned: bool,
}

impl JsonDataProvide {
    pub fn new(file_path: PathBuf) -> Self {
        Self {
            file_path,
            lock: None,
            #[cfg(unix)]
            directory: None,
            snapshot: None,
            versioned: false,
        }
    }

    fn open_store(&mut self) -> anyhow::Result<()> {
        if let Some(lock) = &self.lock {
            let metadata = fs::symlink_metadata(self.lock_path()?)
                .context("Entries lock disappeared; restart the app before writing")?;
            ensure!(metadata.is_file(), "Entries lock must be a regular file");
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let held = lock.metadata()?;
                ensure!(
                    metadata.dev() == held.dev() && metadata.ino() == held.ino(),
                    "Entries lock was replaced; restart the app before writing"
                );
                let directory = fs::metadata(self.parent())?;
                let held = self
                    .directory
                    .as_ref()
                    .context("Missing entries directory handle")?
                    .metadata()?;
                ensure!(
                    directory.dev() == held.dev() && directory.ino() == held.ino(),
                    "Entries directory was replaced; restart the app before writing"
                );
            }
            #[cfg(not(unix))]
            let _ = lock;
            return Ok(());
        }
        let parent = self.parent();
        crate::persistence::create_private_dirs(parent)
            .with_context(|| format!("Failed to create entries directory: {}", parent.display()))?;
        let parent = fs::canonicalize(parent).context("Resolving entries directory")?;
        let name = self
            .file_path
            .file_name()
            .context("Missing entries filename")?
            .to_owned();
        self.file_path = parent.join(&name);
        let lock_path = self.lock_path()?;
        if let Ok(metadata) = fs::symlink_metadata(&lock_path) {
            ensure!(metadata.is_file(), "Entries lock must be a regular file");
        }
        let mut options = OpenOptions::new();
        options.read(true).write(true).create(true).truncate(false);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let lock = options.open(&lock_path).context("Opening entries lock")?;
        lock.try_lock().map_err(|error| {
            anyhow::anyhow!("Cannot lock entries file; close any other app using it: {error}")
        })?;
        #[cfg(unix)]
        {
            self.directory = Some(File::open(&parent).context("Opening entries directory")?);
        }
        self.lock = Some(lock);
        self.open_store()
    }

    fn lock_path(&self) -> anyhow::Result<PathBuf> {
        let mut name = self
            .file_path
            .file_name()
            .context("Missing entries filename")?
            .to_owned();
        name.push(".lock");
        Ok(self.parent().join(name))
    }

    fn remember_bytes(&mut self, bytes: Option<Vec<u8>>) {
        if let Some(Some(previous)) = &mut self.snapshot {
            previous.zeroize();
        }
        self.snapshot = Some(bytes);
    }

    fn parent(&self) -> &Path {
        self.file_path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
    }

    fn read_bytes(&self) -> anyhow::Result<Option<Vec<u8>>> {
        match fs::symlink_metadata(&self.file_path) {
            Ok(metadata) => ensure!(metadata.is_file(), "Entries path must be a regular file"),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(error).context("Inspecting entries file"),
        }
        fs::read(&self.file_path)
            .map(Some)
            .with_context(|| format!("Failed to read entries file: {}", self.file_path.display()))
    }

    fn decode(&mut self, bytes: Option<&[u8]>) -> anyhow::Result<Vec<Entry>> {
        let Some(bytes) = bytes.filter(|bytes| !bytes.is_empty()) else {
            return Ok(Vec::new());
        };
        let value: serde_json::Value = serde_json::from_slice(bytes).with_context(|| {
            format!("Failed to parse entries file: {}", self.file_path.display())
        })?;
        let entries: Vec<Entry> = if value.is_array() {
            ensure!(
                !self.versioned,
                "Entries file format was downgraded; reload from a valid backup"
            );
            let entries: Vec<Entry> = serde_json::from_value(value)?;
            ensure!(
                entries.iter().all(|entry| entry.protection.is_none()),
                "Protected notes require a versioned entries file"
            );
            entries
        } else {
            let store: VersionedStore = serde_json::from_value(value)?;
            ensure!(
                store.format == "tui-journal" && store.version == PROTECTED_STORE_VERSION,
                "Unsupported entries file format or version"
            );
            self.versioned = true;
            store.entries
        };
        let mut ids = std::collections::HashSet::new();
        for entry in &entries {
            entry.validate_protection()?;
            ensure!(ids.insert(entry.id), "Entries file contains duplicate ids");
        }
        Ok(entries)
    }

    fn read_for_write(&mut self) -> anyhow::Result<Vec<Entry>> {
        self.open_store()?;
        let bytes = self.read_bytes()?;
        if let Some(snapshot) = &self.snapshot {
            ensure!(
                &bytes == snapshot,
                "Entries file changed outside the app; reload before writing"
            );
        }
        let entries = self.decode(bytes.as_deref())?;
        self.remember_bytes(bytes);
        Ok(entries)
    }

    fn write_entries_to_file(&mut self, entries: Vec<Entry>) -> anyhow::Result<()> {
        for entry in &entries {
            entry.validate_protection()?;
        }
        let versioned = self.versioned || entries.iter().any(|entry| entry.protection.is_some());
        let bytes = if versioned {
            serde_json::to_vec(&VersionedStore {
                format: "tui-journal".into(),
                version: PROTECTED_STORE_VERSION,
                entries,
            })?
        } else {
            serde_json::to_vec(&entries)?
        };
        let mut temporary = tempfile::NamedTempFile::new_in(self.parent())
            .context("Creating temporary entries file")?;
        if let Ok(metadata) = fs::metadata(&self.file_path) {
            temporary
                .as_file()
                .set_permissions(metadata.permissions())?;
        }
        temporary
            .write_all(&bytes)
            .context("Writing entries file")?;
        temporary.flush().context("Flushing entries file")?;
        temporary
            .as_file()
            .sync_all()
            .context("Syncing entries file")?;
        self.open_store()?;
        ensure!(
            Some(self.read_bytes()?) == self.snapshot,
            "Entries file changed outside the app; reload before writing"
        );
        temporary
            .persist(&self.file_path)
            .context("Replacing entries file")?;
        self.versioned = versioned;
        self.remember_bytes(Some(bytes));
        #[cfg(unix)]
        if let Some(directory) = &self.directory
            && let Err(error) = directory.sync_all()
        {
            log::warn!("Entries file was committed, but syncing its directory failed: {error}");
        }
        Ok(())
    }
}

impl Drop for JsonDataProvide {
    fn drop(&mut self) {
        if let Some(Some(bytes)) = &mut self.snapshot {
            bytes.zeroize();
        }
    }
}

impl DataProvider for JsonDataProvide {
    async fn load_all_entries(&mut self) -> anyhow::Result<Vec<Entry>> {
        self.open_store()?;
        let bytes = self.read_bytes()?;
        let entries = self.decode(bytes.as_deref())?;
        self.remember_bytes(bytes);
        Ok(entries)
    }

    async fn add_entry(&mut self, entry: EntryDraft) -> Result<Entry, ModifyEntryError> {
        validate_title(&entry.title)?;
        let mut entries = self.read_for_write()?;
        let id = match entries.iter().map(|entry| entry.id).max() {
            Some(id) => id.checked_add(1).context("No entry ids remain")?,
            None => 0,
        };
        let new_entry = Entry::from_draft(id, entry);
        new_entry.validate_protection()?;
        entries.push(new_entry.clone());
        self.write_entries_to_file(entries)?;
        Ok(new_entry)
    }

    async fn restore_entry(&mut self, entry: Entry) -> Result<Entry, ModifyEntryError> {
        validate_title(&entry.title)?;
        entry.validate_protection()?;
        let mut entries = self.read_for_write()?;
        if entries.iter().any(|existing| existing.id == entry.id) {
            return Err(ModifyEntryError::ValidationError(format!(
                "Entry id {} already exists",
                entry.id
            )));
        }
        entries.push(entry.clone());
        self.write_entries_to_file(entries)?;
        Ok(entry)
    }

    async fn remove_entry(&mut self, entry_id: u32) -> anyhow::Result<()> {
        let mut entries = self.read_for_write()?;
        if let Some(pos) = entries.iter().position(|entry| entry.id == entry_id) {
            entries.remove(pos);
            self.write_entries_to_file(entries)?;
        }
        Ok(())
    }

    async fn update_entry(&mut self, entry: Entry) -> Result<Entry, ModifyEntryError> {
        validate_title(&entry.title)?;
        entry.validate_protection()?;
        let mut entries = self.read_for_write()?;
        let existing = entries
            .iter_mut()
            .find(|existing| existing.id == entry.id)
            .ok_or_else(|| {
                ModifyEntryError::ValidationError(format!("Entry {} does not exist", entry.id))
            })?;
        *existing = entry.clone();
        self.write_entries_to_file(entries)?;
        Ok(entry)
    }

    async fn get_export_object(&mut self, entries_ids: &[u32]) -> anyhow::Result<EntriesDTO> {
        let entries = self
            .read_for_write()?
            .into_iter()
            .filter(|entry| entries_ids.contains(&entry.id))
            .map(EntryDraft::from_entry)
            .collect();
        Ok(EntriesDTO::new(entries))
    }

    async fn import_entries(&mut self, entries_dto: EntriesDTO) -> anyhow::Result<()> {
        entries_dto.validate()?;
        if entries_dto.entries.is_empty() {
            return Ok(());
        }
        let mut entries = self.read_for_write()?;
        let next_id = entries
            .iter()
            .map(|entry| u64::from(entry.id) + 1)
            .max()
            .unwrap_or(0);
        for (next_id, draft) in (next_id..).zip(entries_dto.entries) {
            let id = u32::try_from(next_id).context("No entry ids remain")?;
            entries.push(Entry::from_draft(id, draft));
        }
        self.write_entries_to_file(entries)
    }

    async fn assign_priority_to_entries(&mut self, priority: u32) -> anyhow::Result<()> {
        let mut entries = self.read_for_write()?;
        let mut modified = false;
        for entry in entries
            .iter_mut()
            .filter(|entry| entry.priority.is_none() && entry.protection.is_none())
        {
            entry.priority = Some(priority);
            modified = true;
        }
        if modified {
            self.write_entries_to_file(entries)?;
        }
        Ok(())
    }
}

fn validate_title(title: &str) -> Result<(), ModifyEntryError> {
    if title.is_empty() {
        return Err(ModifyEntryError::ValidationError(
            "Entry title can't be empty".into(),
        ));
    }
    Ok(())
}

use std::{
    fs::{self, File, Metadata, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::settings::{BackendType, Settings};

const PROFILE_FORMAT: &str = "myna-journal-profile";
const METADATA_FILE: &str = "journal.toml";
const MAX_METADATA_BYTES: u64 = 64 * 1024;
const DELETED_DIRECTORY: &str = "Deleted Journals";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Journal {
    pub id: String,
    pub name: String,
    pub settings_path: PathBuf,
    pub is_personal: bool,
}

#[derive(Clone, Debug)]
pub struct JournalStore {
    settings_path: PathBuf,
    root: PathBuf,
    themes_path: PathBuf,
}

pub struct JournalLease {
    _lock: File,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalRecord {
    format: String,
    version: u32,
    id: String,
    name: String,
    personal_settings_path: PathBuf,
    themes_path: PathBuf,
}

struct CatalogGuard {
    lock: File,
    root: PathBuf,
    identity: FileIdentity,
}

#[derive(PartialEq, Eq)]
struct FileIdentity {
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
    #[cfg(not(unix))]
    created: Option<std::time::SystemTime>,
}

impl FileIdentity {
    fn of(metadata: &Metadata) -> Self {
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

impl JournalStore {
    pub fn new(settings_path: PathBuf) -> Result<Self> {
        let settings_path = std::path::absolute(settings_path)?;
        let parent = settings_path
            .parent()
            .context("Configuration has no directory")?;
        let parent = resolve_path(parent)?;
        let filename = settings_path
            .file_name()
            .context("Configuration has no filename")?;
        let namespace = if filename == "config.toml" {
            "journals".to_owned()
        } else {
            let digest = Sha256::digest(filename.as_encoded_bytes());
            format!("journals-{:x}", digest)
        };
        let store = Self {
            settings_path,
            root: parent.join(namespace),
            themes_path: parent.join("themes"),
        };
        store.root_exists()?;
        Ok(store)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn themes_path(&self) -> &Path {
        &self.themes_path
    }

    pub fn with_themes_path(mut self, path: PathBuf) -> Result<Self> {
        self.themes_path = std::path::absolute(path)?;
        Ok(self)
    }

    pub fn list(&self) -> Result<Vec<Journal>> {
        if !self.root_exists()? {
            return Ok(vec![self.personal()]);
        }
        let guard = self.catalog_guard()?;
        let journals = self.list_locked()?;
        guard.verify()?;
        Ok(journals)
    }

    pub fn create(&self, name: &str, base: &Settings) -> Result<Journal> {
        let name = valid_name(name)?;
        let guard = self.catalog_guard()?;
        ensure!(
            !self
                .list_locked()?
                .iter()
                .any(|journal| journal.name.to_lowercase() == name.to_lowercase()),
            "A journal named {name:?} already exists"
        );
        let id = loop {
            let id = format!("j_{:032x}", rand::random::<u128>());
            if !path_present(&self.root.join(&id))?
                && !path_present(&self.root.join(DELETED_DIRECTORY).join(&id))?
            {
                break id;
            }
        };
        let destination = self.root.join(&id);
        let mut staging = tempfile::Builder::new();
        staging.prefix(".new-journal-");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            staging.permissions(fs::Permissions::from_mode(0o700));
        }
        let staged = staging.tempdir_in(&self.root)?;
        for directory in ["data", "state", "exports"] {
            private_directories(&staged.path().join(directory))?;
        }
        let mut settings = base.clone();
        profile_paths(&mut settings, &destination, true);
        write_new_private(
            &staged.path().join("config.toml"),
            toml::to_string(&settings)?.as_bytes(),
        )?;
        let record = JournalRecord {
            format: PROFILE_FORMAT.into(),
            version: 1,
            id: id.clone(),
            name: name.clone(),
            personal_settings_path: self.settings_path.clone(),
            themes_path: self.themes_path.clone(),
        };
        write_new_private(
            &staged.path().join(METADATA_FILE),
            toml::to_string(&record)?.as_bytes(),
        )?;
        sync_directory(staged.path())?;
        guard.verify()?;
        ensure!(
            !path_present(&destination)?,
            "Journal destination appeared while creating it"
        );
        fs::rename(staged.path(), &destination).context("Publishing new journal failed")?;
        let _ = staged.keep();
        sync_committed_directory(&self.root);
        Ok(Journal {
            id,
            name,
            settings_path: destination.join("config.toml"),
            is_personal: false,
        })
    }

    pub fn load_settings(&self, journal: &Journal) -> Result<Settings> {
        self.validate_journal(journal)?;
        if journal.is_personal {
            return match fs::read_to_string(&journal.settings_path) {
                Ok(text) => toml::from_str(&text).context("Reading Personal configuration failed"),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    Ok(Settings::default())
                }
                Err(error) => Err(error).context("Opening Personal configuration failed"),
            };
        }
        let text = read_regular(&journal.settings_path, None)?;
        let mut settings = toml::from_str(&text).context("Reading journal configuration failed")?;
        apply_profile_defaults(&mut settings, &journal.settings_path)?;
        Ok(settings)
    }

    pub fn acquire(&self, journal: &Journal) -> Result<JournalLease> {
        let guard = self.catalog_guard()?;
        self.validate_journal(journal)?;
        let lease = self.acquire_locked(journal)?;
        guard.verify()?;
        Ok(lease)
    }

    pub fn archive(&self, journal: &Journal) -> Result<PathBuf> {
        ensure!(
            !journal.is_personal,
            "Personal contains your existing files and cannot be deleted"
        );
        let guard = self.catalog_guard()?;
        self.validate_journal(journal)?;
        let _lease = self.acquire_locked(journal)?;
        let profile = self.root.join(&journal.id);
        self.ensure_owned_paths(journal, &profile)?;
        let deleted = self.root.join(DELETED_DIRECTORY);
        private_directories(&deleted)?;
        let destination = deleted.join(&journal.id);
        ensure!(
            !path_present(&destination)?,
            "A deleted journal already occupies this recovery path"
        );
        guard.verify()?;
        require_directory(&profile)?;
        fs::rename(&profile, &destination).context("Moving journal to Deleted Journals failed")?;
        sync_committed_directory(&deleted);
        sync_committed_directory(&self.root);
        Ok(destination)
    }

    fn personal(&self) -> Journal {
        Journal {
            id: "personal".into(),
            name: "Personal".into(),
            settings_path: self.settings_path.clone(),
            is_personal: true,
        }
    }

    fn root_exists(&self) -> Result<bool> {
        match fs::symlink_metadata(&self.root) {
            Ok(metadata) => {
                ensure!(
                    metadata.is_dir() && !metadata.is_symlink(),
                    "Journal collection must be a real directory: {}",
                    self.root.display()
                );
                ensure!(
                    fs::canonicalize(&self.root)? == self.root,
                    "Journal collection path was redirected"
                );
                Ok(true)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(error).context("Inspecting journal collection failed"),
        }
    }

    fn catalog_guard(&self) -> Result<CatalogGuard> {
        ensure!(
            resolve_path(self.root.parent().unwrap())? == self.root.parent().unwrap(),
            "Journal collection parent was redirected"
        );
        private_directories(&self.root)?;
        let identity = FileIdentity::of(&require_directory(&self.root)?);
        let lock = open_lock(&self.root.join(".catalog.lock"))?;
        lock.try_lock()
            .context("Another journal selector is updating this collection; retry shortly")?;
        let guard = CatalogGuard {
            lock,
            root: self.root.clone(),
            identity,
        };
        guard.verify()?;
        Ok(guard)
    }

    fn list_locked(&self) -> Result<Vec<Journal>> {
        let mut journals = Vec::new();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            if valid_id(&id) {
                journals.push(self.read_journal(&id)?);
            }
        }
        journals.sort_by(|left, right| {
            (left.name.to_lowercase(), &left.id).cmp(&(right.name.to_lowercase(), &right.id))
        });
        let mut names = std::collections::HashSet::from(["personal".to_owned()]);
        for journal in &journals {
            ensure!(
                names.insert(journal.name.to_lowercase()),
                "Duplicate journal name in collection: {}",
                journal.name
            );
        }
        journals.insert(0, self.personal());
        Ok(journals)
    }

    fn read_journal(&self, id: &str) -> Result<Journal> {
        ensure!(valid_id(id), "Invalid journal identifier");
        self.root_exists()?
            .then_some(())
            .context("Journal collection is missing")?;
        let profile = self.root.join(id);
        require_directory(&profile)?;
        let record = read_record(&profile.join(METADATA_FILE))?;
        ensure!(
            record.id == id,
            "Journal metadata does not match its directory"
        );
        let owner = JournalStore::new(record.personal_settings_path.clone())?;
        ensure!(
            owner.root == self.root,
            "Journal belongs to a different collection"
        );
        Ok(Journal {
            id: id.to_owned(),
            name: record.name,
            settings_path: profile.join("config.toml"),
            is_personal: false,
        })
    }

    fn validate_journal(&self, journal: &Journal) -> Result<()> {
        let expected = if journal.is_personal {
            self.personal()
        } else {
            self.read_journal(&journal.id)?
        };
        ensure!(
            journal == &expected,
            "Journal changed or does not belong to this collection; reopen the selector"
        );
        Ok(())
    }

    fn acquire_locked(&self, journal: &Journal) -> Result<JournalLease> {
        let leases = self.root.join(".leases");
        private_directories(&leases)?;
        let identity = FileIdentity::of(&require_directory(&leases)?);
        let lock = open_lock(&leases.join(format!("{}.lock", journal.id)))?;
        lock.try_lock().with_context(|| {
            format!(
                "Journal {:?} is open in another window; close it before continuing",
                journal.name
            )
        })?;
        ensure!(
            FileIdentity::of(&require_directory(&leases)?) == identity,
            "Journal lease directory changed"
        );
        Ok(JournalLease { _lock: lock })
    }

    fn ensure_owned_paths(&self, journal: &Journal, profile: &Path) -> Result<()> {
        let text = read_regular(&journal.settings_path, None)?;
        let raw: toml::Value =
            toml::from_str(&text).context("Reading journal configuration failed")?;
        let mut settings: Settings = toml::from_str(&text)?;
        profile_paths(&mut settings, profile, false);
        let completed = toml::Value::try_from(&settings)?;
        for (section, key) in [
            (Some("json_backend"), "file_path"),
            (Some("sqlite_backend"), "file_path"),
            (Some("vjournal_backend"), "directory"),
            (Some("file_backend"), "directory"),
            (Some("export"), "default_path"),
            (None, "app_state_dir"),
        ] {
            let at = |document: &toml::Value| {
                let table = match section {
                    Some(section) => document.get(section)?,
                    None => document,
                };
                table.get(key).cloned()
            };
            if let Some(value) = at(&raw).or_else(|| at(&completed)) {
                let path = value
                    .as_str()
                    .context("Journal storage path is not a string")?;
                ensure!(
                    resolve_path(Path::new(path))?.starts_with(profile),
                    "This journal uses files outside its managed folder; they have been preserved. Change its storage paths before deleting it: {path}"
                );
            }
        }
        Ok(())
    }
}

/// Recognize only profiles whose metadata and collection-derived path agree.
pub fn journal_for_config(config_path: &Path) -> Result<Option<(JournalStore, Journal)>> {
    let absolute = std::path::absolute(config_path)?;
    reject_archived_storage(&absolute)?;
    let resolved = resolve_path(&absolute)?;
    reject_archived_storage(&resolved)?;
    let mut candidate = absolute;
    for _ in 0..40 {
        reject_archived_storage(&candidate)?;
        let Some(filename) = candidate.file_name() else {
            return Ok(None);
        };
        let parent = candidate
            .parent()
            .context("Configuration has no directory")?;
        let location = resolve_path(parent)?.join(filename);
        reject_archived_storage(&location)?;
        // Validate every link's location before following a config-file alias:
        // a managed config must itself remain a regular file in its profile.
        if let Some(profile) = journal_at_config_location(&location)? {
            return Ok(Some(profile));
        }
        let is_alias = match fs::symlink_metadata(&location) {
            Ok(metadata) => metadata.is_symlink(),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
            Err(error) => return Err(error.into()),
        };
        if !is_alias {
            return if resolved != location {
                journal_at_config_location(&resolved)
            } else {
                Ok(None)
            };
        }
        let target = fs::read_link(&location)?;
        candidate = if target.is_absolute() {
            target
        } else {
            location.parent().unwrap().join(target)
        };
    }
    bail!("Too many configuration file aliases")
}

fn journal_at_config_location(config_path: &Path) -> Result<Option<(JournalStore, Journal)>> {
    if config_path
        .file_name()
        .is_none_or(|name| name != "config.toml")
    {
        return Ok(None);
    }
    let parent = config_path
        .parent()
        .context("Configuration has no directory")?;
    let metadata_path = parent.join(METADATA_FILE);
    if !path_present(&metadata_path)? {
        return Ok(None);
    }
    if !parent
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(valid_id)
    {
        // Ordinary config directories may contain an unrelated journal.toml.
        // A profile moved or renamed outside the managed namespace still has
        // to pass the full ownership validation when its format identifies it.
        let Ok(text) = read_regular(&metadata_path, Some(MAX_METADATA_BYTES)) else {
            return Ok(None);
        };
        let metadata = toml::from_str::<toml::Value>(&text).ok();
        if metadata
            .as_ref()
            .and_then(|value| value.get("format"))
            .and_then(toml::Value::as_str)
            != Some(PROFILE_FORMAT)
        {
            return Ok(None);
        }
    }
    require_directory(parent)?;
    let record = read_record(&metadata_path)?;
    let store =
        JournalStore::new(record.personal_settings_path)?.with_themes_path(record.themes_path)?;
    let journal = store.read_journal(&record.id)?;
    ensure!(
        resolve_path(parent)? == journal.settings_path.parent().unwrap(),
        "Journal profile is outside its original collection; restore it before opening"
    );
    require_regular(config_path)?;
    Ok(Some((store, journal)))
}

/// Find the journal owning an explicitly selected backend path without creating files.
pub fn journal_for_storage(settings: &Settings) -> Result<Option<(JournalStore, Journal)>> {
    let path = match settings.backend_type.unwrap_or_default() {
        #[cfg(feature = "json")]
        BackendType::Json => settings
            .json_backend
            .file_path
            .clone()
            .map_or_else(crate::settings::json_backend::get_default_json_path, Ok)?,
        #[cfg(feature = "sqlite")]
        BackendType::Sqlite => settings
            .sqlite_backend
            .file_path
            .clone()
            .map_or_else(crate::settings::sqlite_backend::get_default_sqlite_path, Ok)?,
        #[cfg(feature = "vjournal")]
        BackendType::Vjournal => settings.vjournal_backend.directory.clone().map_or_else(
            crate::settings::vjournal_backend::get_default_vjournal_path,
            Ok,
        )?,
        #[cfg(feature = "file")]
        BackendType::File => settings
            .file_backend
            .directory
            .clone()
            .map_or_else(crate::settings::file_backend::get_default_file_path, Ok)?,
        #[allow(unreachable_patterns)]
        _ => bail!("This build does not include the selected journal backend"),
    };
    let absolute = std::path::absolute(path)?;
    reject_archived_storage(&absolute)?;
    let resolved = resolve_path(&absolute)?;
    reject_archived_storage(&resolved)?;
    for ancestor in resolved.ancestors() {
        if ancestor
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(valid_id)
            && path_present(&ancestor.join(METADATA_FILE))?
        {
            return journal_for_config(&ancestor.join("config.toml"));
        }
    }
    Ok(None)
}

fn reject_archived_storage(path: &Path) -> Result<()> {
    for ancestor in path.ancestors() {
        let Some(id) = ancestor
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|id| valid_id(id))
        else {
            continue;
        };
        let Some(parent) = ancestor.parent() else {
            continue;
        };
        let (collection, archived) = if parent
            .file_name()
            .is_some_and(|name| name == DELETED_DIRECTORY)
        {
            (
                parent
                    .parent()
                    .context("Deleted journal has no collection")?,
                ancestor.to_owned(),
            )
        } else {
            (parent, parent.join(DELETED_DIRECTORY).join(id))
        };
        let metadata = archived.join(METADATA_FILE);
        if !path_present(&metadata)? {
            continue;
        }
        require_directory(archived.parent().unwrap())?;
        require_directory(&archived)?;
        let record = read_record(&metadata)?;
        let store = JournalStore::new(record.personal_settings_path)?;
        if record.id == id && store.root == resolve_path(collection)? {
            bail!(
                "Journal {:?} has been deleted; restore it from Deleted Journals before opening",
                record.name
            );
        }
    }
    Ok(())
}

/// Keep reset-to-default settings inside the selected managed journal.
pub fn apply_profile_defaults(settings: &mut Settings, config_path: &Path) -> Result<bool> {
    let Some((_store, journal)) = journal_for_config(config_path)? else {
        return Ok(false);
    };
    profile_paths(settings, journal.settings_path.parent().unwrap(), false);
    Ok(true)
}

fn profile_paths(settings: &mut Settings, root: &Path, replace: bool) {
    let fill = |field: &mut Option<PathBuf>, relative| {
        if replace || field.is_none() {
            *field = Some(root.join(relative));
        }
    };
    #[cfg(feature = "json")]
    fill(&mut settings.json_backend.file_path, "data/entries.json");
    #[cfg(feature = "sqlite")]
    fill(&mut settings.sqlite_backend.file_path, "data/entries.db");
    #[cfg(feature = "vjournal")]
    fill(&mut settings.vjournal_backend.directory, "data/journal");
    #[cfg(feature = "file")]
    fill(&mut settings.file_backend.directory, "data/entries");
    fill(&mut settings.app_state_dir, "state");
    fill(&mut settings.export.default_path, "exports");
}

fn valid_name(name: &str) -> Result<String> {
    ensure!(
        !name.chars().any(char::is_control),
        "Journal names cannot contain control characters"
    );
    let name = name.trim();
    ensure!(!name.is_empty(), "Enter a journal name");
    ensure!(
        name.chars().count() <= 80,
        "Journal names must be at most 80 characters"
    );
    Ok(name.to_owned())
}

fn valid_id(id: &str) -> bool {
    id.len() == 34
        && id.starts_with("j_")
        && id.as_bytes()[2..]
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

fn read_record(path: &Path) -> Result<JournalRecord> {
    let record: JournalRecord = toml::from_str(&read_regular(path, Some(MAX_METADATA_BYTES))?)
        .with_context(|| format!("Reading journal metadata failed: {}", path.display()))?;
    ensure!(
        record.format == PROFILE_FORMAT && record.version == 1,
        "Unsupported journal metadata"
    );
    ensure!(valid_id(&record.id), "Invalid journal identifier");
    ensure!(
        valid_name(&record.name)? == record.name,
        "Journal metadata contains an invalid name"
    );
    ensure!(
        record.personal_settings_path.is_absolute(),
        "Journal collection path must be absolute"
    );
    ensure!(
        record.themes_path.is_absolute(),
        "Journal themes path must be absolute"
    );
    Ok(record)
}

impl CatalogGuard {
    fn verify(&self) -> Result<()> {
        ensure!(
            FileIdentity::of(&require_directory(&self.root)?) == self.identity,
            "Journal collection directory changed"
        );
        ensure!(
            FileIdentity::of(&require_regular(&self.root.join(".catalog.lock"))?)
                == FileIdentity::of(&self.lock.metadata()?),
            "Journal collection lock changed"
        );
        Ok(())
    }
}

fn path_present(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn require_directory(path: &Path) -> Result<Metadata> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("Inspecting directory {}", path.display()))?;
    ensure!(
        metadata.is_dir() && !metadata.is_symlink(),
        "Journal directory must not be a symbolic link: {}",
        path.display()
    );
    Ok(metadata)
}

fn require_regular(path: &Path) -> Result<Metadata> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("Inspecting journal file {}", path.display()))?;
    ensure!(
        metadata.is_file() && !metadata.is_symlink(),
        "Journal metadata and locks must be regular files: {}",
        path.display()
    );
    Ok(metadata)
}

fn read_regular(path: &Path, limit: Option<u64>) -> Result<String> {
    let original = require_regular(path)?;
    let file = File::open(path)?;
    ensure!(
        FileIdentity::of(&original) == FileIdentity::of(&file.metadata()?),
        "Journal file changed while opening"
    );
    let mut text = String::new();
    file.take(limit.unwrap_or(u64::MAX).saturating_add(1))
        .read_to_string(&mut text)?;
    if let Some(limit) = limit {
        ensure!(text.len() as u64 <= limit, "Journal metadata is too large");
    }
    let current = require_regular(path)?;
    ensure!(
        FileIdentity::of(&current) == FileIdentity::of(&original)
            && current.len() == original.len()
            && current.modified().ok() == original.modified().ok(),
        "Journal file changed while reading"
    );
    Ok(text)
}

fn open_lock(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    if path_present(path)? {
        require_regular(path)?;
    } else {
        options.create_new(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)?;
    ensure!(
        FileIdentity::of(&require_regular(path)?) == FileIdentity::of(&file.metadata()?),
        "Journal lock changed while opening"
    );
    Ok(file)
}

fn private_directories(path: &Path) -> Result<()> {
    if path_present(path)? {
        require_directory(path)?;
        return Ok(());
    }
    let parent = path.parent().context("Journal directory has no parent")?;
    private_directories(parent)?;
    let mut builder = fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    match builder.create(path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error.into()),
    }
    require_directory(path)?;
    Ok(())
}

fn write_new_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn sync_committed_directory(path: &Path) {
    if let Err(error) = sync_directory(path) {
        log::warn!(
            "Journal change succeeded, but flushing directory {} failed: {error:#}",
            path.display()
        );
    }
}

fn resolve_path(path: &Path) -> Result<PathBuf> {
    let path = std::path::absolute(path)?;
    match fs::canonicalize(&path) {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ensure!(
                !path_present(&path)?,
                "Path contains a dangling symbolic link: {}",
                path.display()
            );
            let parent = path.parent().context("Path has no parent directory")?;
            let name = path.file_name().context("Path has no filename")?;
            Ok(resolve_path(parent)?.join(name))
        }
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests;

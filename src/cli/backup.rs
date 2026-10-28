//! Local native-format backups. Restore always creates a separate profile.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, Metadata, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use clap::Subcommand;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    app::{state::AppState, ui::themes::ThemeStore},
    settings::{BackendType, Settings},
};

const FORMAT: &str = "myna-journal/native-backup";
const VERSION: u32 = 1;
const MANIFEST: &str = "manifest.json";
const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;
const CONFIG: &str = "config/config.toml";
const ORIGINAL_CONFIG: &str = "config/original.toml";

#[derive(Debug, Clone, Subcommand, PartialEq, Eq)]
pub enum BackupCommand {
    /// Back up the selected backend, settings, themes, and state into a new directory.
    Create { directory: PathBuf },
    /// Verify a backup and restore it into a new, separate profile.
    Restore {
        directory: PathBuf,
        #[arg(long, value_name = "NEW_PROFILE")]
        to: PathBuf,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: String,
    version: u32,
    backend: BackendType,
    created_at: String,
    directories: BTreeSet<String>,
    files: BTreeMap<String, FileRecord>,
}

impl Manifest {
    fn encode(&self) -> Result<Vec<u8>> {
        let bytes = serde_json::to_vec_pretty(self)?;
        ensure!(
            bytes.len() as u64 <= MAX_MANIFEST_BYTES,
            "Backup manifest exceeds the 16 MiB limit; reduce the number or length of source paths"
        );
        Ok(bytes)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct FileRecord {
    bytes: u64,
    sha256: String,
}

#[derive(Clone, Debug)]
struct Source {
    path: PathBuf,
    destination: PathBuf,
    directory: bool,
}

#[derive(Debug)]
struct Inventory {
    files: BTreeMap<PathBuf, FileRecord>,
    directories: BTreeSet<PathBuf>,
}

struct OwnedDirectory {
    path: PathBuf,
    identity: Metadata,
    complete: bool,
}

impl OwnedDirectory {
    fn create(path: &Path) -> Result<Self> {
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(path)
            .with_context(|| format!("Destination must be a new directory: {}", path.display()))?;
        Ok(Self {
            path: path.to_path_buf(),
            identity: fs::metadata(path)?,
            complete: false,
        })
    }

    fn finish(mut self) -> Result<()> {
        let current = fs::symlink_metadata(&self.path)?;
        ensure!(
            current.is_dir() && same_identity(&current, &self.identity),
            "Destination directory was replaced while writing"
        );
        sync_directory(&self.path)?;
        sync_directory(self.path.parent().context("Destination has no parent")?)?;
        self.complete = true;
        Ok(())
    }
}

impl Drop for OwnedDirectory {
    fn drop(&mut self) {
        if !self.complete
            && fs::symlink_metadata(&self.path)
                .is_ok_and(|metadata| metadata.is_dir() && same_identity(&metadata, &self.identity))
        {
            let _ = fs::remove_dir_all(&self.path);
        }
    }
}

fn data_relative(backend: BackendType) -> &'static str {
    match backend {
        BackendType::Json => "data/entries.json",
        BackendType::Sqlite => "data/entries.db",
        BackendType::File => "data/entries",
        BackendType::Vjournal => "data/journal",
    }
}

fn source_data(settings: &Settings) -> Result<Source> {
    let backend = settings.backend_type.unwrap_or_default();
    let path = match backend {
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
        #[cfg(feature = "file")]
        BackendType::File => settings
            .file_backend
            .directory
            .clone()
            .map_or_else(crate::settings::file_backend::get_default_file_path, Ok)?,
        #[cfg(feature = "vjournal")]
        BackendType::Vjournal => settings.vjournal_backend.directory.clone().map_or_else(
            crate::settings::vjournal_backend::get_default_vjournal_path,
            Ok,
        )?,
        #[allow(unreachable_patterns)]
        _ => anyhow::bail!("This build does not include the selected backup backend"),
    };
    Ok(Source {
        path: resolve_root(&path)?,
        destination: data_relative(backend).into(),
        directory: matches!(backend, BackendType::File | BackendType::Vjournal),
    })
}

fn resolve_root(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    match fs::canonicalize(&absolute) {
        Ok(path) => Ok(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ensure!(
                fs::symlink_metadata(&absolute).is_err(),
                "Source contains a dangling symbolic link: {}",
                absolute.display()
            );
            let parent = absolute.parent().context("Path has no parent")?;
            let name = absolute.file_name().context("Path has no filename")?;
            Ok(resolve_root(parent)?.join(name))
        }
        Err(error) => Err(error.into()),
    }
}

pub async fn create(
    settings: &Settings,
    custom_config: Option<&PathBuf>,
    destination: &Path,
) -> Result<()> {
    let backend = settings.backend_type.unwrap_or_default();
    let data = source_data(settings)?;
    let config_path = Settings::file_path(custom_config.cloned())?;
    let theme_store = ThemeStore::new(custom_config)?;
    let state_dir = settings
        .app_state_dir
        .clone()
        .map_or_else(AppState::default_persist_dir, Ok)?;
    let mut sources = vec![
        Source {
            path: resolve_root(&config_path)?,
            destination: ORIGINAL_CONFIG.into(),
            directory: false,
        },
        Source {
            path: resolve_root(theme_store.directory())?,
            destination: "config/themes".into(),
            directory: true,
        },
        Source {
            path: resolve_root(&state_dir.join("state.json"))?,
            destination: "state/state.json".into(),
            directory: false,
        },
    ];
    if let Some(parent) = theme_store.directory().parent() {
        sources.push(Source {
            path: resolve_root(&parent.join("themes.toml"))?,
            destination: "config/themes.toml".into(),
            directory: false,
        });
    }
    let destination = resolve_root(destination)?;
    for source in sources.iter().chain(std::iter::once(&data)) {
        ensure!(
            destination != source.path
                && !(source.directory && destination.starts_with(&source.path)),
            "Backup destination must be outside source data, themes, settings, and state"
        );
    }
    // Protect the whole settings/state directory, including files not normally copied.
    for directory in [config_path.parent(), Some(state_dir.as_path())]
        .into_iter()
        .flatten()
    {
        ensure!(
            !destination.starts_with(resolve_root(directory)?),
            "Backup destination must be outside the source configuration/state directory"
        );
    }
    let _locks = acquire_source_locks(&data, backend)?;
    let identities: Vec<_> = sources
        .iter()
        .chain(std::iter::once(&data))
        .map(|source| match fs::symlink_metadata(&source.path) {
            Ok(metadata) => Ok(Some(metadata)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        })
        .collect::<std::io::Result<_>>()?;
    let before: Vec<_> = sources
        .iter()
        .map(inventory_source)
        .collect::<Result<_>>()?;
    let data_before = if backend == BackendType::Sqlite {
        None
    } else {
        Some(inventory_source(&data)?)
    };
    let output = OwnedDirectory::create(&destination)?;
    for (source, inventory) in sources.iter().zip(&before) {
        copy_inventory(source, inventory, &destination)?;
    }
    if backend == BackendType::Sqlite {
        snapshot_sqlite(&data.path, &destination.join(&data.destination)).await?;
    } else {
        copy_inventory(&data, data_before.as_ref().unwrap(), &destination)?;
    }
    let raw_config = match fs::read_to_string(destination.join(ORIGINAL_CONFIG)) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error.into()),
    };
    let mut document = raw_config.parse::<toml_edit::DocumentMut>()?;
    document["backend_type"] = toml_edit::value(backend_name(backend));
    write_private_new(&destination.join(CONFIG), document.to_string().as_bytes())?;
    for (source, original) in sources.iter().zip(&before) {
        ensure!(
            inventory_source(source)?.same_contents(original),
            "Source changed during backup: {}; close the app and retry",
            source.path.display()
        );
    }
    if let Some(original) = data_before {
        ensure!(
            inventory_source(&data)?.same_contents(&original),
            "Journal data changed during backup; close the app and retry"
        );
    }
    for (source, identity) in sources
        .iter()
        .chain(std::iter::once(&data))
        .zip(&identities)
    {
        let unchanged = match (identity, fs::symlink_metadata(&source.path)) {
            (Some(before), Ok(after)) => !after.is_symlink() && same_identity(before, &after),
            (None, Err(error)) => error.kind() == std::io::ErrorKind::NotFound,
            _ => false,
        };
        ensure!(
            unchanged,
            "Source was replaced during backup: {}",
            source.path.display()
        );
    }
    let snapshot = inventory_tree(&destination, false)?;
    let manifest = Manifest {
        format: FORMAT.into(),
        version: VERSION,
        backend,
        created_at: chrono::Utc::now().to_rfc3339(),
        directories: snapshot
            .directories
            .iter()
            .map(|path| portable_path(path))
            .collect::<Result<_>>()?,
        files: snapshot
            .files
            .into_iter()
            .map(|(path, record)| Ok((portable_path(&path)?, record)))
            .collect::<Result<_>>()?,
    };
    for directory in &manifest.directories {
        sync_directory(&destination.join(directory))?;
    }
    write_private_new(&destination.join(MANIFEST), &manifest.encode()?)?;
    output.finish()?;
    Ok(())
}

pub fn restore(backup: &Path, destination: &Path) -> Result<()> {
    let backup = fs::canonicalize(backup).context("Opening backup directory")?;
    let backup_identity = fs::symlink_metadata(&backup)?;
    ensure!(backup_identity.is_dir(), "Backup must be a directory");
    let manifest_path = backup.join(MANIFEST);
    let manifest_metadata =
        fs::symlink_metadata(&manifest_path).context("Backup has no completed manifest")?;
    ensure!(
        manifest_metadata.is_file() && manifest_metadata.len() <= MAX_MANIFEST_BYTES,
        "Invalid backup manifest"
    );
    let mut manifest_bytes = Vec::new();
    open_regular(&manifest_path)?
        .take(MAX_MANIFEST_BYTES + 1)
        .read_to_end(&mut manifest_bytes)?;
    ensure!(
        manifest_bytes.len() as u64 <= MAX_MANIFEST_BYTES,
        "Invalid backup manifest"
    );
    let manifest: Manifest = serde_json::from_slice(&manifest_bytes)?;
    ensure!(
        manifest.format == FORMAT && manifest.version == VERSION,
        "Unsupported backup format or version"
    );
    let backend_available = match manifest.backend {
        BackendType::Json => cfg!(feature = "json"),
        BackendType::Sqlite => cfg!(feature = "sqlite"),
        BackendType::File => cfg!(feature = "file"),
        BackendType::Vjournal => cfg!(feature = "vjournal"),
    };
    ensure!(
        backend_available,
        "This build does not include the backup's backend"
    );
    for path in manifest.files.keys().chain(manifest.directories.iter()) {
        validate_relative(path)?;
    }
    ensure!(
        !manifest.files.contains_key(MANIFEST),
        "Manifest cannot list itself"
    );
    let mut actual = inventory_tree(&backup, false)?;
    let manifest_record = actual
        .files
        .remove(Path::new(MANIFEST))
        .context("Backup manifest disappeared")?;
    ensure!(
        hash_reader(&mut manifest_bytes.as_slice())? == manifest_record,
        "Backup manifest changed while being read"
    );
    let expected_files: BTreeMap<_, _> = manifest
        .files
        .iter()
        .map(|(path, record)| (PathBuf::from(path), record.clone()))
        .collect();
    let expected_dirs: BTreeSet<_> = manifest.directories.iter().map(PathBuf::from).collect();
    ensure!(
        actual.files == expected_files && actual.directories == expected_dirs,
        "Backup contents do not match the manifest (missing, extra, or changed files)"
    );
    ensure!(
        expected_files.contains_key(Path::new(CONFIG)),
        "Backup is missing its configuration"
    );
    let destination = resolve_root(destination)?;
    ensure!(
        !destination.starts_with(&backup),
        "Restore destination must be outside the backup"
    );
    let output = OwnedDirectory::create(&destination)?;
    for directory in &actual.directories {
        private_dirs(&destination.join(directory))?;
    }
    for (relative, record) in &actual.files {
        copy_file_checked(&backup.join(relative), &destination.join(relative), record)?;
    }
    let config_text = fs::read_to_string(destination.join(CONFIG))?;
    let mut document = config_text.parse::<toml_edit::DocumentMut>()?;
    document["backend_type"] = toml_edit::value(backend_name(manifest.backend));
    document["app_state_dir"] = path_value(&destination.join("state"))?;
    for (section, key, relative) in [
        ("json_backend", "file_path", "data/entries.json"),
        ("sqlite_backend", "file_path", "data/entries.db"),
        ("file_backend", "directory", "data/entries"),
        ("vjournal_backend", "directory", "data/journal"),
    ] {
        if document.get(section).is_none() {
            document[section] = toml_edit::Item::Table(toml_edit::Table::new());
        }
        ensure!(
            document[section].is_table_like(),
            "Invalid backend section in backup configuration"
        );
        document[section][key] = path_value(&destination.join(relative))?;
    }
    private_dirs(&destination.join("state"))?;
    private_dirs(&destination.join("config/themes"))?;
    // This file belongs to the just-created profile, not an existing user profile.
    let config_path = destination.join(CONFIG);
    let mut config = OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&config_path)?;
    config.write_all(document.to_string().as_bytes())?;
    config.sync_all()?;
    let mut final_inventory = inventory_tree(&backup, false)?;
    ensure!(
        same_identity(&backup_identity, &fs::symlink_metadata(&backup)?)
            && final_inventory.files.remove(Path::new(MANIFEST)) == Some(manifest_record)
            && final_inventory.same_contents(&actual),
        "Backup changed while restoring; no profile was restored"
    );
    for directory in &actual.directories {
        sync_directory(&destination.join(directory))?;
    }
    output.finish()?;
    Ok(())
}

fn backend_name(backend: BackendType) -> &'static str {
    match backend {
        BackendType::Json => "Json",
        BackendType::Sqlite => "Sqlite",
        BackendType::File => "File",
        BackendType::Vjournal => "Vjournal",
    }
}

fn path_value(path: &Path) -> Result<toml_edit::Item> {
    Ok(toml_edit::value(
        path.to_str()
            .context("Restored profile path must be valid UTF-8")?,
    ))
}

impl Inventory {
    fn same_contents(&self, other: &Self) -> bool {
        self.files == other.files && self.directories == other.directories
    }
}

fn inventory_source(source: &Source) -> Result<Inventory> {
    if !source.path.try_exists()? {
        return Ok(Inventory {
            files: BTreeMap::new(),
            directories: BTreeSet::new(),
        });
    }
    if source.directory {
        ensure!(
            source.path.is_dir(),
            "Expected a directory: {}",
            source.path.display()
        );
        inventory_tree(&source.path, true)
    } else {
        Ok(Inventory {
            files: BTreeMap::from([(PathBuf::new(), hash_file(&source.path)?)]),
            directories: BTreeSet::new(),
        })
    }
}

fn inventory_tree(root: &Path, skip_locks: bool) -> Result<Inventory> {
    fn walk(root: &Path, relative: &Path, output: &mut Inventory, skip_locks: bool) -> Result<()> {
        for entry in fs::read_dir(root.join(relative))? {
            let entry = entry?;
            let path = relative.join(entry.file_name());
            portable_path(&path)?;
            let metadata = fs::symlink_metadata(entry.path())?;
            ensure!(
                !metadata.is_symlink(),
                "Symbolic links are not permitted inside a backup source: {}",
                entry.path().display()
            );
            if skip_locks && entry.file_name() == ".tui-journal.lock" {
                continue;
            }
            if metadata.is_dir() {
                output.directories.insert(path.clone());
                walk(root, &path, output, skip_locks)?;
            } else {
                ensure!(
                    metadata.is_file(),
                    "Only regular files and directories can be backed up"
                );
                output.files.insert(path, hash_file(&entry.path())?);
            }
        }
        Ok(())
    }
    let mut output = Inventory {
        files: BTreeMap::new(),
        directories: BTreeSet::new(),
    };
    walk(root, Path::new(""), &mut output, skip_locks)?;
    Ok(output)
}

fn copy_inventory(source: &Source, inventory: &Inventory, destination: &Path) -> Result<()> {
    let output = destination.join(&source.destination);
    if source.directory && source.path.exists() {
        private_dirs(&output)?;
    }
    for directory in &inventory.directories {
        private_dirs(&output.join(directory))?;
    }
    for (relative, record) in &inventory.files {
        let from = if relative.as_os_str().is_empty() {
            source.path.clone()
        } else {
            source.path.join(relative)
        };
        let to = if relative.as_os_str().is_empty() {
            output.clone()
        } else {
            output.join(relative)
        };
        copy_file_checked(&from, &to, record)?;
    }
    Ok(())
}

fn open_regular(path: &Path) -> Result<File> {
    let before = fs::symlink_metadata(path)?;
    ensure!(
        before.is_file(),
        "Expected a regular file: {}",
        path.display()
    );
    let file = File::open(path)?;
    ensure!(
        same_identity(&before, &file.metadata()?),
        "File changed while opening: {}",
        path.display()
    );
    Ok(file)
}

fn hash_reader(reader: &mut impl Read) -> Result<FileRecord> {
    let mut hash = Sha256::new();
    let mut bytes = 0;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = reader.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
        bytes += count as u64;
    }
    Ok(FileRecord {
        bytes,
        sha256: format!("{:x}", hash.finalize()),
    })
}

fn hash_file(path: &Path) -> Result<FileRecord> {
    hash_reader(&mut open_regular(path)?)
}

fn copy_file_checked(source: &Path, destination: &Path, expected: &FileRecord) -> Result<()> {
    let mut input = open_regular(source)?;
    let initial = input.metadata()?;
    let mut output = private_file(destination)?;
    std::io::copy(&mut input, &mut output)?;
    output.sync_all()?;
    ensure!(
        same_identity(&initial, &fs::symlink_metadata(source)?),
        "Source file was replaced during backup"
    );
    ensure!(
        hash_file(destination)? == *expected,
        "Source file changed while copying: {}",
        source.display()
    );
    Ok(())
}

fn private_dirs(path: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)?;
    Ok(())
}

fn private_file(path: &Path) -> Result<File> {
    private_dirs(path.parent().context("Missing destination parent")?)?;
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    Ok(options.open(path)?)
}

fn write_private_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = private_file(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    Ok(())
}

fn same_identity(left: &Metadata, right: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        left.dev() == right.dev() && left.ino() == right.ino()
    }
    #[cfg(not(unix))]
    {
        left.is_dir() == right.is_dir() && left.created().ok() == right.created().ok()
    }
}

fn sync_directory(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path)?.sync_all()?;
    #[cfg(not(unix))]
    let _ = path;
    Ok(())
}

fn portable_path(path: &Path) -> Result<String> {
    let text = path.to_str().context("Backup paths must be valid UTF-8")?;
    validate_relative(text)?;
    Ok(text.to_owned())
}

fn validate_relative(text: &str) -> Result<()> {
    ensure!(
        !text.is_empty() && !text.contains(['\\', ':']) && !text.chars().any(char::is_control),
        "Invalid backup path"
    );
    ensure!(
        text.split('/')
            .all(|part| !part.is_empty() && part != "." && part != ".."),
        "Invalid backup path"
    );
    ensure!(
        Path::new(text)
            .components()
            .all(|part| matches!(part, Component::Normal(_))),
        "Backup paths must be relative"
    );
    Ok(())
}

fn acquire_source_locks(data: &Source, backend: BackendType) -> Result<Vec<File>> {
    let mut paths = Vec::new();
    if backend == BackendType::Json {
        let mut path = data.path.as_os_str().to_os_string();
        path.push(".lock");
        paths.push(PathBuf::from(path));
    } else if data.directory && data.path.is_dir() {
        paths.push(data.path.join(".tui-journal.lock"));
        for directory in inventory_tree(&data.path, true)?.directories {
            paths.push(data.path.join(directory).join(".tui-journal.lock"));
        }
    }
    let mut locks = Vec::new();
    for path in paths {
        if !path.try_exists()? {
            continue;
        }
        let lock = open_regular(&path)?;
        lock.try_lock().map_err(|_| {
            anyhow::anyhow!("Journal store is in use; close the app before backing up")
        })?;
        ensure!(
            same_identity(&lock.metadata()?, &fs::symlink_metadata(&path)?),
            "Journal lock was replaced"
        );
        locks.push(lock);
    }
    Ok(locks)
}

#[cfg(feature = "sqlite")]
async fn snapshot_sqlite(source: &Path, destination: &Path) -> Result<()> {
    use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
    if !source.try_exists()? {
        return Ok(());
    }
    let identity = open_regular(source)?.metadata()?;
    let options = SqliteConnectOptions::new()
        .filename(source)
        .read_only(true)
        .create_if_missing(false);
    let mut connection = SqliteConnection::connect_with(&options).await?;
    private_file(destination)?;
    let result = sqlx::query("VACUUM INTO ?")
        .bind(
            destination
                .to_str()
                .context("SQLite backup path must be UTF-8")?,
        )
        .execute(&mut connection)
        .await;
    connection.close().await?;
    result.context("Creating a consistent SQLite snapshot")?;
    ensure!(
        same_identity(&identity, &fs::symlink_metadata(source)?),
        "SQLite source was replaced during backup"
    );
    File::open(destination)?.sync_all()?;
    Ok(())
}

#[cfg(not(feature = "sqlite"))]
async fn snapshot_sqlite(_source: &Path, _destination: &Path) -> Result<()> {
    anyhow::bail!("This build cannot back up SQLite databases")
}

#[cfg(test)]
mod tests;

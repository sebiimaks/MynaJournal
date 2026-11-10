//! Verified native-storage relocation. Configuration changes only after publication.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use backend::journal::{JournalLease, JournalProvider};

use crate::{
    app::state::AppState,
    cli::backup::{self, Inventory, Source},
    journal_storage::{is_protection_file, is_within_directory},
    settings::{BackendType, ConfigContext, Settings, document::ConfigDocument},
};

#[derive(Debug, Clone)]
pub struct MoveRequest {
    pub source: PathBuf,
    pub destination: PathBuf,
    pub storage_destination: PathBuf,
    settings: Settings,
    context: ConfigContext,
    document: ConfigDocument,
    source_snapshot: Snapshot,
    header_snapshot: Snapshot,
    sqlite_snapshot: Vec<(Source, Snapshot)>,
    parent_identity: fs::Metadata,
}

#[derive(Debug)]
pub struct MoveReport {
    pub destination: PathBuf,
    pub recovery_path: PathBuf,
    pub warning: Option<String>,
}

#[derive(Debug, Clone)]
struct Snapshot {
    identity: Option<fs::Metadata>,
    inventory: Inventory,
}

impl Snapshot {
    fn read(source: &Source) -> Result<Self> {
        let identity = metadata(&source.path)?;
        if let Some(metadata) = &identity {
            ensure!(
                !metadata.is_symlink(),
                "Journal storage must not be a symbolic link"
            );
            ensure!(
                metadata.is_dir() == source.directory && (metadata.is_dir() || metadata.is_file()),
                "Unexpected journal storage type"
            );
        }
        let inventory = if source.directory && identity.is_some() {
            // Relocation preserves every file, including native locks and unrelated files.
            backup::inventory_tree(&source.path, false)?
        } else {
            backup::inventory_source(source)?
        };
        let snapshot = Self {
            identity,
            inventory,
        };
        snapshot.verify_identity(source)?;
        Ok(snapshot)
    }

    fn verify_identity(&self, source: &Source) -> Result<()> {
        let current = metadata(&source.path)?;
        let unchanged = match (&self.identity, current) {
            (None, None) => true,
            (Some(before), Some(after)) => {
                !after.is_symlink() && backup::same_identity(before, &after)
            }
            _ => false,
        };
        ensure!(
            unchanged,
            "Journal storage was replaced: {}",
            source.path.display()
        );
        Ok(())
    }

    fn verify(&self, source: &Source) -> Result<()> {
        self.verify_identity(source)?;
        ensure!(
            Self::read(source)?.inventory.same_contents(&self.inventory),
            "Journal storage changed; reopen it before moving: {}",
            source.path.display()
        );
        Ok(())
    }
}

pub fn prepare(
    settings: &Settings,
    context: &ConfigContext,
    document: &ConfigDocument,
    destination: &Path,
) -> Result<MoveRequest> {
    ensure!(
        supported_platform(),
        "Move journal is supported on macOS and Linux x86-64/ARM64"
    );
    ensure!(
        !context.storage_overridden,
        "Reopen this journal without command-line storage overrides before moving it"
    );
    ensure!(
        !document.is_dirty(),
        "Save or discard configuration edits before moving the journal"
    );
    document.verify_unchanged()?;
    ensure!(
        backup::resolve_root(document.path())? == backup::resolve_root(&context.settings_path)?,
        "Configuration does not belong to this journal"
    );
    let saved = Settings::from_config_text(&document.text(), document.path())?;
    ensure!(
        settings.backend_type.unwrap_or_default() == saved.backend_type.unwrap_or_default(),
        "Storage settings have changed; reopen the journal before moving it"
    );
    #[cfg(feature = "file")]
    if settings.backend_type.unwrap_or_default() == BackendType::File {
        ensure!(
            settings.file_backend.format == saved.file_backend.format,
            "File format changed; reopen the journal before moving it"
        );
    }
    let backend = crate::journal_storage::backend(settings)?;
    let saved_backend = crate::journal_storage::backend(&saved)?;
    let source_metadata =
        fs::symlink_metadata(backend.path()).context("Journal storage does not exist yet")?;
    ensure!(
        !source_metadata.is_symlink(),
        "Journal storage must not be a symbolic link; use the real location"
    );
    let source = fs::canonicalize(backend.path())?;
    ensure!(
        source == backup::resolve_root(saved_backend.path())?,
        "Active storage differs from saved settings; reopen the journal before moving it"
    );
    if let Some((_, owner)) = crate::journals::journal_for_storage(settings)? {
        ensure!(
            backup::resolve_root(&owner.settings_path)?
                == backup::resolve_root(&context.settings_path)?,
            "Current storage belongs to another journal; open its owning journal before moving it"
        );
    }
    ensure!(
        !is_protection_file(&source),
        "Choose the journal storage, not a protection sidecar"
    );
    ensure!(
        !path_present(&backend.pending_path())?,
        "Journal conversion is incomplete; reopen it to recover before moving"
    );
    JournalProvider::status(&backend)?;

    ensure!(
        destination.is_absolute(),
        "Enter an absolute path for the new destination folder"
    );
    ensure!(
        !destination
            .as_os_str()
            .to_string_lossy()
            .chars()
            .any(char::is_control),
        "Destination must not contain control characters"
    );
    ensure!(
        !path_present(destination)?,
        "Destination must be a new folder; existing locations are never replaced"
    );
    let name = destination
        .file_name()
        .context("Destination must have a folder name")?;
    let parent = fs::canonicalize(destination.parent().context("Destination has no parent")?)
        .context("The destination's parent folder must already exist")?;
    ensure!(parent.is_dir(), "Destination parent must be a directory");
    let destination = parent.join(name);
    ensure!(!path_present(&destination)?, "Destination already exists");
    ensure!(
        !is_protection_file(&destination),
        "Destination must not use a journal protection filename"
    );
    ensure!(
        !is_within_directory(&destination, &source) && !is_within_directory(&source, &destination),
        "Destination must be outside the current journal storage"
    );
    let storage_destination = destination.join(
        source
            .file_name()
            .context("Journal storage has no filename")?,
    );
    ensure!(
        storage_destination.to_str().is_some(),
        "Storage path must be valid UTF-8"
    );
    let mut moved = settings.clone();
    set_storage(&mut moved, storage_destination.clone());
    if let Some((_, owner)) = crate::journals::journal_for_storage(&moved)? {
        ensure!(
            backup::resolve_root(&owner.settings_path)?
                == backup::resolve_root(&context.settings_path)?,
            "Destination belongs to another journal"
        );
    }

    let mut reserved = vec![context.themes_path.clone()];
    reserved.push(
        settings
            .app_state_dir
            .clone()
            .map_or_else(AppState::default_persist_dir, Ok)?,
    );
    if let Some(path) = &settings.export.default_path {
        reserved.push(path.clone());
    }
    for path in &mut reserved {
        *path = backup::resolve_root(path)?;
        ensure!(
            !is_within_directory(&destination, path),
            "Destination must be outside themes, state and export locations"
        );
    }
    reserved.push(backup::resolve_root(&context.settings_path)?);
    for path in reserved {
        ensure!(
            !is_within_directory(&path, &source),
            "Journal storage contains configuration, themes, state or export paths; move those settings outside it first"
        );
        ensure!(
            !is_within_directory(&path, &destination),
            "Destination contains an application configuration or state path"
        );
    }
    for path in inactive_storage(settings)? {
        let path = backup::resolve_root(&path)?;
        ensure!(
            !is_within_directory(&destination, &path) && !is_within_directory(&path, &destination),
            "Destination overlaps another configured backend location"
        );
        ensure!(
            !path_present(&path)?
                || (!is_within_directory(&path, &source) && !is_within_directory(&source, &path)),
            "Current storage overlaps another backend's files; separate the backend locations before moving"
        );
    }
    let (data, header) = sources(&source, settings)?;
    Ok(MoveRequest {
        source,
        destination,
        storage_destination,
        settings: settings.clone(),
        context: context.clone(),
        document: document.clone(),
        source_snapshot: Snapshot::read(&data)?,
        header_snapshot: Snapshot::read(&header)?,
        sqlite_snapshot: sqlite_sidecars(&data.path, settings.backend_type.unwrap_or_default())?,
        parent_identity: fs::metadata(parent)?,
    })
}

pub async fn execute(mut request: MoveRequest) -> Result<MoveReport> {
    let checked = prepare(
        &request.settings,
        &request.context,
        &request.document,
        &request.destination,
    )?;
    ensure!(
        checked.source == request.source
            && checked.storage_destination == request.storage_destination,
        "Journal location changed while confirming the move"
    );
    let backend_type = request.settings.backend_type.unwrap_or_default();
    let mut source_settings = request.settings.clone();
    set_storage(&mut source_settings, request.source.clone());
    let backend = crate::journal_storage::backend(&source_settings)?;
    let _journal_lease = JournalLease::acquire(&backend)?;
    let (data, header) = sources(&request.source, &source_settings)?;
    let _native_locks = backup::acquire_source_locks(&data, backend_type)?;
    let sqlite_guard = SqliteMoveGuard::acquire(&data.path, backend_type).await?;
    let mut destination_sqlite_guard = None;
    let result: Result<MoveReport> = async {
        request.source_snapshot.verify(&data)?;
        request.header_snapshot.verify(&header)?;
        verify_sidecars(&request.sqlite_snapshot)?;
        let before = Snapshot::read(&data)?;
        let sqlite_sidecars = sqlite_sidecars(&data.path, backend_type)?;
        let parent = request.destination.parent().context("Destination has no parent")?;
        verify_parent(parent, &request.parent_identity)?;
        let staged = private_staging(parent, ".myna-move-")?;
        let staged_data = staged.path().join(&data.destination);
        if backend_type == BackendType::Sqlite {
            backup::snapshot_sqlite(&data.path, &staged_data).await?;
            verify_sqlite(&staged_data).await?;
        } else {
            backup::copy_inventory(&data, &before.inventory, staged.path())?;
            let copied = Source { path: staged_data.clone(), ..data.clone() };
            ensure!(Snapshot::read(&copied)?.inventory.same_contents(&before.inventory), "Copied journal failed verification");
        }
        backup::copy_inventory(&header, &request.header_snapshot.inventory, staged.path())?;
        let copied_header = Source { path: staged.path().join(&header.destination), ..header.clone() };
        ensure!(Snapshot::read(&copied_header)?.inventory.same_contents(&request.header_snapshot.inventory), "Copied protection header failed verification");
        for directory in backup::inventory_tree(staged.path(), false)?.directories {
            backup::sync_directory(&staged.path().join(directory))?;
        }
        backup::sync_directory(staged.path())?;
        let destination_snapshot = Snapshot::read(&Source { path: staged_data, ..data.clone() })?;
        let destination_header_snapshot = Snapshot::read(&copied_header)?;
        let bundle_identity = fs::metadata(staged.path())?;
        before.verify(&data)?;
        request.header_snapshot.verify(&header)?;
        verify_sidecars(&sqlite_sidecars)?;
        request.document.verify_unchanged()?;
        verify_parent(parent, &request.parent_identity)?;
        rename_noclobber(staged.path(), &request.destination).context("Publishing verified journal destination")?;
        let _ = staged.keep();
        backup::sync_directory(parent).with_context(|| format!("Original journal remains active; verified copy is at {}", request.destination.display()))?;
        let mut destination_settings = request.settings.clone();
        set_storage(&mut destination_settings, request.storage_destination.clone());
        let destination_backend = crate::journal_storage::backend(&destination_settings)?;
        let _destination_lease = JournalLease::acquire(&destination_backend)
            .with_context(|| format!("Original journal remains active; destination is in use at {}", request.destination.display()))?;
        ensure_native_lock(&request.storage_destination, backend_type)?;
        let (destination_data, destination_header) = sources(&request.storage_destination, &destination_settings)?;
        let _destination_native_locks = backup::acquire_source_locks(&destination_data, backend_type)?;
        destination_sqlite_guard = Some(SqliteMoveGuard::acquire(&destination_data.path, backend_type).await?);
        let config_target = backup::resolve_root(request.document.path())?;

        let commit = (|| -> Result<()> {
            verify_parent(parent, &request.parent_identity)?;
            verify_parent(&request.destination, &bundle_identity)?;
            destination_snapshot.verify(&destination_data)?;
            destination_header_snapshot.verify(&destination_header)?;
            before.verify(&data)?;
            request.header_snapshot.verify(&header)?;
            verify_sidecars(&sqlite_sidecars)?;
            request.document.verify_unchanged()?;
            let path = request.storage_destination.to_str().context("Storage path must be valid UTF-8")?;
            request.document.set(&storage_key(backend_type), Some(path.into()))?;
            request.document.set(&["backend_type"], Some(format!("{backend_type:?}").into()))?;
            request.document.save()
        })();
        commit.with_context(|| format!("Move was not activated; originals remain at {} and the verified copy is at {}", request.source.display(), request.destination.display()))?;

        let mut warning = request.document.verify_unchanged()
            .and_then(|()| backup::sync_directory(config_target.parent().context("Configuration has no parent")?))
            .err().map(|error| format!("The move was saved, but checking or flushing configuration failed: {error}. Original files were retained at {}", request.source.display()));
        let mut recovery_path = request.source.clone();
        if warning.is_none() {
            match preserve_originals(&data, &header, &before, &request.header_snapshot, &sqlite_sidecars) {
                Ok((path, problem)) => { recovery_path = path; warning = problem; }
                Err(error) => warning = Some(format!("New location is active. Original recovery files were retained: {error:#}")),
            }
        }
        Ok(MoveReport { destination: request.storage_destination, recovery_path, warning })
    }.await;
    let destination_closed = match destination_sqlite_guard {
        Some(guard) => guard.close().await,
        None => Ok(()),
    };
    let source_closed = sqlite_guard.close().await;
    match (result, destination_closed.and(source_closed)) {
        (Ok(mut report), Err(error)) => {
            let message = format!(
                "New location is active; closing the original SQLite connection reported: {error:#}"
            );
            report.warning = Some(
                report
                    .warning
                    .map_or(message.clone(), |old| format!("{old}\n{message}")),
            );
            Ok(report)
        }
        (Err(error), Err(close)) => Err(error.context(format!(
            "Closing the SQLite move lock also failed: {close:#}"
        ))),
        (result, Ok(())) => result,
    }
}

fn inactive_storage(settings: &Settings) -> Result<Vec<PathBuf>> {
    let active = settings.backend_type.unwrap_or_default();
    let mut paths = Vec::new();
    #[cfg(feature = "json")]
    if active != BackendType::Json {
        paths.push(
            settings
                .json_backend
                .file_path
                .clone()
                .map_or_else(crate::settings::json_backend::get_default_json_path, Ok)?,
        );
    }
    #[cfg(feature = "sqlite")]
    if active != BackendType::Sqlite {
        paths.push(
            settings
                .sqlite_backend
                .file_path
                .clone()
                .map_or_else(crate::settings::sqlite_backend::get_default_sqlite_path, Ok)?,
        );
    }
    #[cfg(feature = "file")]
    if active != BackendType::File {
        paths.push(
            settings
                .file_backend
                .directory
                .clone()
                .map_or_else(crate::settings::file_backend::get_default_file_path, Ok)?,
        );
    }
    #[cfg(feature = "vjournal")]
    if active != BackendType::Vjournal {
        paths.push(settings.vjournal_backend.directory.clone().map_or_else(
            crate::settings::vjournal_backend::get_default_vjournal_path,
            Ok,
        )?);
    }
    let sidecars: Vec<_> = paths
        .iter()
        .flat_map(|path| {
            [
                ".myna-lock.json",
                ".myna-lock.pending.json",
                ".myna-lock.lease",
                ".lock",
                "-wal",
                "-shm",
                "-journal",
            ]
            .map(|suffix| {
                let mut name = path.as_os_str().to_owned();
                name.push(suffix);
                PathBuf::from(name)
            })
        })
        .collect();
    paths.extend(sidecars);
    Ok(paths)
}

fn ensure_native_lock(path: &Path, backend: BackendType) -> Result<()> {
    if backend == BackendType::Json {
        let mut lock = path.as_os_str().to_owned();
        lock.push(".lock");
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        match options.open(PathBuf::from(lock)) {
            Ok(file) => file.sync_all()?,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

struct SqliteMoveGuard {
    #[cfg(feature = "sqlite")]
    connection: Option<sqlx::SqliteConnection>,
}

impl SqliteMoveGuard {
    async fn acquire(path: &Path, backend: BackendType) -> Result<Self> {
        #[cfg(feature = "sqlite")]
        {
            use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
            if backend == BackendType::Sqlite {
                let options = SqliteConnectOptions::new()
                    .filename(path)
                    .create_if_missing(false)
                    .busy_timeout(std::time::Duration::ZERO);
                let mut connection = SqliteConnection::connect_with(&options).await?;
                let mode: String = sqlx::query_scalar("PRAGMA journal_mode")
                    .fetch_one(&mut connection)
                    .await?;
                ensure!(
                    mode.eq_ignore_ascii_case("delete"),
                    "Close other SQLite programs and reopen this journal before moving it; DELETE journal mode is required"
                );
                sqlx::query("BEGIN IMMEDIATE")
                    .execute(&mut connection)
                    .await
                    .context("SQLite journal is in use; close other programs before moving it")?;
                return Ok(Self {
                    connection: Some(connection),
                });
            }
            Ok(Self { connection: None })
        }
        #[cfg(not(feature = "sqlite"))]
        {
            let _ = (path, backend);
            Ok(Self {})
        }
    }

    async fn close(self) -> Result<()> {
        #[cfg(feature = "sqlite")]
        if let Some(mut connection) = self.connection {
            use sqlx::Connection;
            let rollback = sqlx::query("ROLLBACK").execute(&mut connection).await;
            let closed = connection.close().await;
            rollback?;
            closed?;
        }
        Ok(())
    }
}

fn sources(path: &Path, settings: &Settings) -> Result<(Source, Source)> {
    let mut settings = settings.clone();
    set_storage(&mut settings, path.to_path_buf());
    let backend = crate::journal_storage::backend(&settings)?;
    let header = backend.header_path();
    Ok((
        Source {
            path: path.to_owned(),
            destination: path.file_name().context("Missing storage filename")?.into(),
            directory: matches!(
                settings.backend_type.unwrap_or_default(),
                BackendType::File | BackendType::Vjournal
            ),
        },
        Source {
            destination: header
                .file_name()
                .context("Missing header filename")?
                .into(),
            path: header,
            directory: false,
        },
    ))
}

fn storage_key(backend: BackendType) -> [&'static str; 2] {
    match backend {
        BackendType::Json => ["json_backend", "file_path"],
        BackendType::Sqlite => ["sqlite_backend", "file_path"],
        BackendType::File => ["file_backend", "directory"],
        BackendType::Vjournal => ["vjournal_backend", "directory"],
    }
}

fn set_storage(settings: &mut Settings, path: PathBuf) {
    match settings.backend_type.unwrap_or_default() {
        #[cfg(feature = "json")]
        BackendType::Json => settings.json_backend.file_path = Some(path),
        #[cfg(feature = "sqlite")]
        BackendType::Sqlite => settings.sqlite_backend.file_path = Some(path),
        #[cfg(feature = "file")]
        BackendType::File => settings.file_backend.directory = Some(path),
        #[cfg(feature = "vjournal")]
        BackendType::Vjournal => settings.vjournal_backend.directory = Some(path),
        #[allow(unreachable_patterns)]
        _ => {}
    }
}

fn metadata(path: &Path) -> Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn path_present(path: &Path) -> Result<bool> {
    Ok(metadata(path)?.is_some())
}

fn verify_parent(path: &Path, original: &fs::Metadata) -> Result<()> {
    let current = fs::symlink_metadata(path)?;
    ensure!(
        current.is_dir()
            && !current.is_symlink()
            && backup::same_identity(original, &current)
            && fs::canonicalize(path)? == path,
        "Destination parent was replaced or redirected"
    );
    Ok(())
}

fn sqlite_sidecars(path: &Path, backend: BackendType) -> Result<Vec<(Source, Snapshot)>> {
    if backend != BackendType::Sqlite {
        return Ok(Vec::new());
    }
    ["-wal", "-shm", "-journal"]
        .into_iter()
        .map(|suffix| {
            let mut name = path.as_os_str().to_owned();
            name.push(suffix);
            let path = PathBuf::from(name);
            let source = Source {
                destination: path.file_name().unwrap().into(),
                path,
                directory: false,
            };
            Ok((source.clone(), Snapshot::read(&source)?))
        })
        .collect()
}

fn verify_sidecars(sidecars: &[(Source, Snapshot)]) -> Result<()> {
    for (source, snapshot) in sidecars {
        snapshot.verify(source)?;
    }
    Ok(())
}

#[cfg(feature = "sqlite")]
async fn verify_sqlite(path: &Path) -> Result<()> {
    use sqlx::{Connection, SqliteConnection, sqlite::SqliteConnectOptions};
    let mut connection = SqliteConnection::connect_with(
        &SqliteConnectOptions::new()
            .filename(path)
            .read_only(true)
            .create_if_missing(false),
    )
    .await?;
    let check: String = sqlx::query_scalar("PRAGMA integrity_check")
        .fetch_one(&mut connection)
        .await?;
    connection.close().await?;
    ensure!(
        check == "ok",
        "Copied SQLite journal failed integrity verification"
    );
    Ok(())
}

#[cfg(not(feature = "sqlite"))]
async fn verify_sqlite(_path: &Path) -> Result<()> {
    anyhow::bail!("SQLite is unavailable in this build")
}

fn preserve_originals(
    data: &Source,
    header: &Source,
    before: &Snapshot,
    header_before: &Snapshot,
    sidecars: &[(Source, Snapshot)],
) -> Result<(PathBuf, Option<String>)> {
    before.verify(data)?;
    header_before.verify(header)?;
    verify_sidecars(sidecars)?;
    let parent = data.path.parent().context("Source has no parent")?;
    let recovery = private_staging(parent, ".myna-move-recovery-")?;
    let recovery = recovery.keep();
    let result = (|| -> Result<()> {
        rename_noclobber(&data.path, &recovery.join(&data.destination))?;
        if header_before.identity.is_some() {
            rename_noclobber(&header.path, &recovery.join(&header.destination))?;
        }
        for (source, snapshot) in sidecars {
            if snapshot.identity.is_some() {
                rename_noclobber(&source.path, &recovery.join(&source.destination))?;
            }
        }
        before.verify(&Source {
            path: recovery.join(&data.destination),
            ..data.clone()
        })?;
        header_before.verify(&Source {
            path: recovery.join(&header.destination),
            ..header.clone()
        })?;
        backup::sync_directory(&recovery)?;
        backup::sync_directory(parent)
    })();
    let warning = result.err().map(|error| {
        format!(
            "New location is active. Check both {} and {} for original recovery files: {error:#}",
            data.path.display(),
            recovery.display()
        )
    });
    Ok((recovery, warning))
}

fn private_staging(parent: &Path, prefix: &str) -> Result<tempfile::TempDir> {
    let mut builder = tempfile::Builder::new();
    builder.prefix(prefix);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        builder.permissions(fs::Permissions::from_mode(0o700));
    }
    Ok(builder.tempdir_in(parent)?)
}

fn supported_platform() -> bool {
    cfg!(any(
        target_os = "macos",
        all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    ))
}

fn rename_noclobber(source: &Path, destination: &Path) -> Result<()> {
    #[cfg(any(
        target_os = "macos",
        all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    ))]
    {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};
        let source = CString::new(source.as_os_str().as_bytes())?;
        let destination = CString::new(destination.as_os_str().as_bytes())?;
        #[cfg(target_os = "macos")]
        let result = {
            unsafe extern "C" {
                fn renamex_np(
                    from: *const std::ffi::c_char,
                    to: *const std::ffi::c_char,
                    flags: std::ffi::c_uint,
                ) -> std::ffi::c_int;
            }
            // Darwin sys/stdio.h: RENAME_EXCL = 4. Both C strings live for the call.
            unsafe { renamex_np(source.as_ptr(), destination.as_ptr(), 4) }
        };
        #[cfg(target_os = "linux")]
        let result = {
            unsafe extern "C" {
                fn syscall(number: std::ffi::c_long, ...) -> std::ffi::c_long;
            }
            #[cfg(target_arch = "x86_64")]
            const RENAMEAT2: std::ffi::c_long = 316;
            #[cfg(target_arch = "aarch64")]
            const RENAMEAT2: std::ffi::c_long = 276;
            // Linux asm/unistd: renameat2; AT_FDCWD=-100, RENAME_NOREPLACE=1.
            // The kernel reads the two live C strings; no Rust memory is written.
            unsafe {
                syscall(
                    RENAMEAT2,
                    -100_i32,
                    source.as_ptr(),
                    -100_i32,
                    destination.as_ptr(),
                    1_u32,
                )
            }
        };
        if result != 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        Ok(())
    }
    #[cfg(not(any(
        target_os = "macos",
        all(
            target_os = "linux",
            any(target_arch = "x86_64", target_arch = "aarch64")
        )
    )))]
    {
        let _ = (source, destination);
        anyhow::bail!("Atomic no-replacement moves are unavailable on this platform")
    }
}

#[cfg(test)]
mod tests;

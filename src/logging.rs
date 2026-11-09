use std::{
    fs::{self, OpenOptions},
    path::{Path, PathBuf},
};

use anyhow::anyhow;
use log::LevelFilter;
use simplelog::{Config, WriteLogger};

const LOG_MAX_SIZE: u64 = 1_000;

pub fn validate_destination(
    path: &Path,
    settings: &crate::settings::Settings,
    config: Option<PathBuf>,
) -> anyhow::Result<()> {
    use crate::settings::{BackendType, Settings};
    let destination = resolve_destination(path)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        anyhow::ensure!(
            !fs::metadata(&destination).is_ok_and(|metadata| metadata.nlink() > 1),
            "The log destination cannot be a hard-linked file"
        );
    }
    anyhow::ensure!(
        !crate::journal_storage::is_protection_file(&destination),
        "The log destination cannot be a journal protection file"
    );
    let config_path = Settings::file_path(config)?;
    let collection = match crate::journals::journal_for_config(&config_path)? {
        Some((store, _)) => store,
        None => crate::journals::JournalStore::new(config_path.clone())?,
    };
    let mut protected = vec![(config_path, false), (collection.root().to_owned(), true)];
    for kind in [
        #[cfg(feature = "json")]
        BackendType::Json,
        #[cfg(feature = "sqlite")]
        BackendType::Sqlite,
        #[cfg(feature = "file")]
        BackendType::File,
        #[cfg(feature = "vjournal")]
        BackendType::Vjournal,
    ] {
        let mut candidate = settings.clone();
        candidate.backend_type = Some(kind);
        let storage = crate::journal_storage::backend(&candidate)?;
        protected.extend(
            storage
                .reserved_paths()
                .into_iter()
                .map(|path| (path, false)),
        );
        protected.push((
            storage.path().to_owned(),
            matches!(kind, BackendType::File | BackendType::Vjournal),
        ));
        for suffix in [".lock", "-journal", "-wal", "-shm"] {
            let mut sidecar = storage.path().as_os_str().to_os_string();
            sidecar.push(suffix);
            protected.push((PathBuf::from(sidecar), false));
        }
    }
    for (path, directory) in protected {
        // An unavailable, unused backend must not prevent logging to a valid
        // unrelated path. The destination itself was fully resolved above.
        let Ok(path) = resolve_destination(&path) else {
            continue;
        };
        #[cfg(unix)]
        let alias = {
            use std::os::unix::fs::MetadataExt;
            match (fs::metadata(&path), fs::metadata(&destination)) {
                (Ok(left), Ok(right)) => left.dev() == right.dev() && left.ino() == right.ino(),
                _ => false,
            }
        };
        #[cfg(not(unix))]
        let alias = false;
        anyhow::ensure!(
            destination != path
                && !(directory && crate::journal_storage::is_within_directory(&destination, &path))
                && !alias,
            "Choose a log destination outside journal storage, protection files and configuration"
        );
    }
    Ok(())
}

fn resolve_destination(path: &Path) -> anyhow::Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            std::path::Component::CurDir => continue,
            std::path::Component::ParentDir => {
                resolved.pop();
            }
            _ => resolved.push(component.as_os_str()),
        }
        match fs::canonicalize(&resolved) {
            Ok(path) => resolved = path,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                anyhow::ensure!(
                    fs::symlink_metadata(&resolved).is_err(),
                    "Log path contains an unresolved symbolic link"
                );
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(resolved)
}

pub fn setup_logging(level: u8, path: Option<PathBuf>) -> anyhow::Result<()> {
    let path = match path {
        Some(path) => path,
        None => get_default_path()?,
    };

    let mut append = false;

    if !path.exists() {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
    } else if let Ok(size) = fs::metadata(&path).map(|data| data.len()) {
        append = size < LOG_MAX_SIZE;
    }

    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(path)?;

    let log_level = u8_to_level(level);
    WriteLogger::init(log_level, Config::default(), file)?;

    log::trace!("Log initialized with level: {log_level}");

    Ok(())
}

pub fn get_default_path() -> anyhow::Result<PathBuf> {
    directories::BaseDirs::new()
        .map(|base_dir| {
            base_dir
                .cache_dir()
                .join("myna-journal")
                .join("myna-journal.log")
        })
        .ok_or_else(|| anyhow!("Log file path couldn't be retrieved"))
}

fn u8_to_level(num: u8) -> LevelFilter {
    match num {
        0 => LevelFilter::Warn,
        1 => LevelFilter::Info,
        2 => LevelFilter::Debug,
        _ => LevelFilter::Trace,
    }
}

#[cfg(test)]
mod tests {
    #[cfg(all(unix, feature = "file"))]
    #[test]
    fn log_destinations_cannot_bypass_directory_guards_with_case_aliases() {
        use super::*;
        use crate::settings::{BackendType, Settings};
        use std::os::unix::fs::MetadataExt;
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config.toml");
        fs::write(&config, "").unwrap();
        let native = directory.path().join("entries");
        fs::create_dir(&native).unwrap();
        let alias = directory.path().join("ENTRIES");
        let original = fs::metadata(&native).unwrap();
        if !fs::metadata(&alias)
            .is_ok_and(|other| original.dev() == other.dev() && original.ino() == other.ino())
        {
            return;
        }
        let mut settings = Settings {
            backend_type: Some(BackendType::File),
            ..Default::default()
        };
        settings.file_backend.directory = Some(native);
        assert!(
            validate_destination(&alias.join("entry"), &settings, Some(config.clone())).is_err()
        );
        let store = crate::journals::JournalStore::new(config.clone()).unwrap();
        fs::create_dir_all(store.root()).unwrap();
        let collection_alias = store.root().with_file_name("JOURNALS");
        assert!(
            validate_destination(
                &collection_alias.join("other-journal/data/entry"),
                &settings,
                Some(config)
            )
            .is_err()
        );
    }

    #[cfg(all(unix, feature = "json", feature = "file"))]
    #[test]
    fn unavailable_inactive_backend_does_not_block_an_unrelated_log() {
        use super::*;
        use crate::settings::{BackendType, Settings};
        let directory = tempfile::tempdir().unwrap();
        let config = directory.path().join("config.toml");
        fs::write(&config, "").unwrap();
        let unavailable = directory.path().join("unavailable");
        std::os::unix::fs::symlink(directory.path().join("missing"), &unavailable).unwrap();
        let mut settings = Settings {
            backend_type: Some(BackendType::Json),
            ..Default::default()
        };
        settings.json_backend.file_path = Some(directory.path().join("entries.json"));
        settings.file_backend.directory = Some(unavailable.clone());
        assert!(
            validate_destination(
                &directory.path().join("app.log"),
                &settings,
                Some(config.clone())
            )
            .is_ok()
        );
        assert!(
            validate_destination(
                &settings.json_backend.file_path.clone().unwrap(),
                &settings,
                Some(config.clone())
            )
            .is_err()
        );
        assert!(
            validate_destination(&unavailable.join("app.log"), &settings, Some(config)).is_err()
        );
    }

    #[test]
    fn default_log_uses_myna_journal_folder_and_filename() {
        let path = super::get_default_path().unwrap();
        assert_eq!(path.file_name().unwrap(), "myna-journal.log");
        assert_eq!(path.parent().unwrap().file_name().unwrap(), "myna-journal");
    }
}

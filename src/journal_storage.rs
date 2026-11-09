use anyhow::Result;
use backend::journal::JournalBackend;

use crate::settings::{BackendType, Settings};

/// Check resolved paths, including directory aliases on case-insensitive filesystems.
pub fn is_within_directory(path: &std::path::Path, directory: &std::path::Path) -> bool {
    if path.starts_with(directory) {
        return true;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let Ok(expected) = std::fs::metadata(directory)
            && expected.is_dir()
        {
            return path.ancestors().any(|ancestor| {
                std::fs::metadata(ancestor).is_ok_and(|actual| {
                    actual.dev() == expected.dev() && actual.ino() == expected.ino()
                })
            });
        }
    }
    false
}

pub fn is_protection_file(path: &std::path::Path) -> bool {
    path.file_name().is_some_and(|name| {
        [
            ".myna-lock.json",
            ".myna-lock.pending.json",
            ".myna-lock.lease",
        ]
        .iter()
        .any(|suffix| {
            let bytes = name.as_encoded_bytes();
            bytes.len() >= suffix.len()
                && bytes[bytes.len() - suffix.len()..].eq_ignore_ascii_case(suffix.as_bytes())
        })
    })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::{ffi::OsString, os::unix::ffi::OsStringExt, path::PathBuf};

    #[test]
    fn protection_paths_do_not_require_unicode_filenames() {
        let path = PathBuf::from(OsString::from_vec(b"\xff.MYNA-LOCK.JSON".to_vec()));
        assert!(is_protection_file(&path));
    }

    #[test]
    fn directory_containment_follows_ancestor_identity() {
        let temporary = tempfile::tempdir().unwrap();
        let directory = temporary.path().join("entries");
        let alias = temporary.path().join("alias");
        std::fs::create_dir(&directory).unwrap();
        std::os::unix::fs::symlink(&directory, &alias).unwrap();
        assert!(is_within_directory(
            &alias.join("new/subdirectory/entry"),
            &directory
        ));
        assert!(!is_within_directory(
            &temporary.path().join("unrelated/entry"),
            &directory
        ));
    }

    #[test]
    fn directory_containment_respects_filesystem_case_sensitivity() {
        use std::os::unix::fs::MetadataExt;
        let temporary = tempfile::tempdir().unwrap();
        let directory = temporary.path().join("entries");
        let other_case = temporary.path().join("ENTRIES");
        std::fs::create_dir(&directory).unwrap();
        std::fs::create_dir_all(&other_case).unwrap();
        let original = std::fs::metadata(&directory).unwrap();
        let other = std::fs::metadata(&other_case).unwrap();
        let aliases = original.dev() == other.dev() && original.ino() == other.ino();
        assert_eq!(
            is_within_directory(&other_case.join("new/subdirectory/entry"), &directory),
            aliases
        );
    }
}

/// Resolve the actual storage, independently of profile preferences or UI state.
pub fn backend(settings: &Settings) -> Result<JournalBackend> {
    Ok(match settings.backend_type.unwrap_or_default() {
        #[cfg(feature = "json")]
        BackendType::Json => JournalBackend::Json(
            settings
                .json_backend
                .file_path
                .clone()
                .map_or_else(crate::settings::json_backend::get_default_json_path, Ok)?,
        ),
        #[cfg(feature = "sqlite")]
        BackendType::Sqlite => JournalBackend::Sqlite(
            settings
                .sqlite_backend
                .file_path
                .clone()
                .map_or_else(crate::settings::sqlite_backend::get_default_sqlite_path, Ok)?,
        ),
        #[cfg(feature = "file")]
        BackendType::File => JournalBackend::File(
            settings
                .file_backend
                .directory
                .clone()
                .map_or_else(crate::settings::file_backend::get_default_file_path, Ok)?,
            settings.file_backend.format,
        ),
        #[cfg(feature = "vjournal")]
        BackendType::Vjournal => {
            JournalBackend::Vjournal(settings.vjournal_backend.directory.clone().map_or_else(
                crate::settings::vjournal_backend::get_default_vjournal_path,
                Ok,
            )?)
        }
        #[allow(unreachable_patterns)]
        _ => anyhow::bail!("This build does not include the selected journal backend"),
    })
}

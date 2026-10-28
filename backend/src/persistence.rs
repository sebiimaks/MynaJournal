#[cfg(any(feature = "file", feature = "vjournal"))]
use anyhow::{Context, Result, ensure};
use std::{fs, path::Path};
#[cfg(any(feature = "file", feature = "vjournal"))]
use std::{io::Write, path::PathBuf};

pub(crate) fn create_private_dirs(path: &Path) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

#[cfg(any(feature = "file", feature = "vjournal"))]
pub(crate) struct StagedFile {
    pub(crate) path: PathBuf,
    pub(crate) temporary: tempfile::NamedTempFile,
}

#[cfg(any(feature = "file", feature = "vjournal"))]
impl StagedFile {
    pub(crate) fn new(path: PathBuf, bytes: &[u8]) -> Result<Self> {
        let mut temporary = tempfile::Builder::new()
            .prefix(".myna-import-")
            .suffix(".tmp")
            .tempfile_in(path.parent().context("Missing import directory")?)?;
        temporary.write_all(bytes)?;
        temporary.as_file().sync_all()?;
        Ok(Self { path, temporary })
    }
}

#[cfg(any(feature = "file", feature = "vjournal"))]
pub(crate) fn publish_new_files(
    files: Vec<StagedFile>,
    mut check_directory: impl FnMut() -> Result<()>,
) -> Result<()> {
    let mut published: Vec<(PathBuf, fs::Metadata)> = Vec::new();
    for staged in files {
        let result = (|| {
            check_directory()?;
            ensure!(
                !staged.path.try_exists()?,
                "Import destination already exists"
            );
            let metadata = staged.temporary.as_file().metadata()?;
            staged
                .temporary
                .persist_noclobber(&staged.path)
                .context("Publishing imported journal")?;
            published.push((staged.path, metadata));
            Ok(())
        })();
        if let Err(error) = result {
            let mut remaining = Vec::new();
            for (path, original) in published.iter().rev() {
                let rollback = (|| -> Result<()> {
                    match fs::symlink_metadata(path) {
                        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                        Err(error) => return Err(error.into()),
                        Ok(current) => {
                            ensure!(
                                same_file(&current, original),
                                "Imported journal changed during rollback"
                            );
                            fs::remove_file(path)?;
                        }
                    }
                    Ok(())
                })();
                if rollback.is_err() {
                    remaining.push(path.display().to_string());
                }
            }
            #[cfg(unix)]
            for directory in published
                .iter()
                .filter_map(|(path, _)| path.parent())
                .collect::<std::collections::HashSet<_>>()
            {
                if fs::File::open(directory)
                    .and_then(|file| file.sync_all())
                    .is_err()
                {
                    log::warn!("Could not flush import directory after rollback");
                }
            }
            if remaining.is_empty() {
                anyhow::bail!("Import failed; no new entries were retained. {error:#}");
            }
            anyhow::bail!(
                "Import failed and {} new file(s) could not be safely rolled back. Review these files before retrying: {}. {error:#}",
                remaining.len(),
                remaining.join(", ")
            );
        }
    }
    Ok(())
}

#[cfg(any(feature = "file", feature = "vjournal"))]
fn same_file(current: &fs::Metadata, original: &fs::Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if current.dev() != original.dev() || current.ino() != original.ino() {
            return false;
        }
    }
    current.is_file()
        && current.len() == original.len()
        && current.modified().ok() == original.modified().ok()
        && current.permissions() == original.permissions()
}

#[cfg(all(test, any(feature = "file", feature = "vjournal")))]
mod tests {
    use super::*;

    #[test]
    fn late_import_failure_removes_only_new_files() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        fs::write(&second, "existing").unwrap();
        let files = vec![
            StagedFile::new(first.clone(), b"first").unwrap(),
            StagedFile::new(second.clone(), b"second").unwrap(),
        ];
        let error = publish_new_files(files, || Ok(())).unwrap_err();
        assert!(error.to_string().contains("no new entries were retained"));
        assert!(!first.exists());
        assert_eq!(fs::read_to_string(second).unwrap(), "existing");
    }

    #[test]
    fn rollback_preserves_externally_changed_files_and_reports_partial_result() {
        let dir = tempfile::tempdir().unwrap();
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        let files = vec![
            StagedFile::new(first.clone(), b"first").unwrap(),
            StagedFile::new(second, b"second").unwrap(),
        ];
        let mut calls = 0;
        let error = publish_new_files(files, || {
            calls += 1;
            if calls == 2 {
                fs::write(&first, "external edit").unwrap();
                anyhow::bail!("Directory changed");
            }
            Ok(())
        })
        .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("Review these files before retrying")
        );
        assert_eq!(fs::read_to_string(first).unwrap(), "external edit");
    }
}

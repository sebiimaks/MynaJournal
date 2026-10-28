use std::{
    fs::{self, File, Metadata},
    io::Write,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use backend::DataProvider;

use super::{App, state::AppState};
use crate::settings::{BackendType, Settings, settings_default_dir_path};

#[derive(Clone, Debug)]
pub(crate) struct ExportTarget {
    path: PathBuf,
    replaced: Option<Metadata>,
    ancestors: std::result::Result<Vec<ExportAncestor>, String>,
}

#[derive(Clone, Debug)]
struct ExportAncestor {
    path: PathBuf,
    identity: Option<Metadata>,
}

impl From<PathBuf> for ExportTarget {
    fn from(path: PathBuf) -> Self {
        match Self::inspect(path.clone()) {
            Ok(mut target) => {
                target.replaced = None;
                target
            }
            Err(error) => Self {
                path,
                replaced: None,
                ancestors: Err(error.to_string()),
            },
        }
    }
}

impl ExportTarget {
    pub fn inspect(path: PathBuf) -> Result<Self> {
        ensure!(!path.as_os_str().is_empty(), "Export path cannot be empty");
        let replaced = match fs::symlink_metadata(&path) {
            Ok(metadata) => {
                ensure!(
                    metadata.is_file(),
                    "Choose a regular export file, not a directory or symbolic link"
                );
                Some(metadata)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        };
        let path = resolve_path(&path)?;
        let ancestors = inspect_ancestors(&path)?;
        Ok(Self {
            path,
            replaced,
            ancestors: Ok(ancestors),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn replaces_file(&self) -> bool {
        self.replaced.is_some()
    }

    pub fn write(&self, write: impl FnOnce(&mut File) -> Result<()>) -> Result<()> {
        ensure!(
            !self.path.as_os_str().is_empty(),
            "Export path cannot be empty"
        );
        let ancestors = self
            .ancestors
            .as_ref()
            .map_err(|error| anyhow::anyhow!("Invalid export destination: {error}"))?;
        check_ancestors(ancestors, false)?;
        let destination = &self.path;
        self.check_destination(destination)?;
        let parent = destination
            .parent()
            .context("Export path has no parent directory")?;
        let mut builder = fs::DirBuilder::new();
        builder.recursive(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder.create(parent)?;
        check_ancestors(ancestors, true)?;
        let created_ancestors = inspect_ancestors(destination)?;
        let parent = fs::canonicalize(parent)?;
        #[cfg(unix)]
        let directory_identity = File::open(&parent)?.metadata()?;
        ensure!(
            destination.parent() == Some(parent.as_path()),
            "Export directory changed; select the destination again"
        );
        let mut staged = tempfile::NamedTempFile::new_in(&parent)?;
        if let Some(metadata) = &self.replaced {
            let mut permissions = metadata.permissions();
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                permissions.set_mode(permissions.mode() & 0o600);
            }
            staged.as_file().set_permissions(permissions)?;
        }
        write(staged.as_file_mut())?;
        staged.flush()?;
        staged.as_file().sync_all()?;
        self.check_destination(destination)?;
        check_ancestors(&created_ancestors, false)?;
        #[cfg(unix)]
        ensure!(
            same_file(&directory_identity, &fs::metadata(&parent)?),
            "Export directory was replaced while writing"
        );
        ensure!(
            fs::canonicalize(&parent)? == parent,
            "Export directory changed while writing"
        );
        if self.replaced.is_some() {
            staged.persist(destination)?;
        } else {
            staged.persist_noclobber(destination)?;
        }
        #[cfg(unix)]
        if File::open(&parent)
            .and_then(|directory| directory.sync_all())
            .is_err()
        {
            log::warn!("Export saved, but flushing its directory failed");
        }
        Ok(())
    }

    fn check_destination(&self, path: &Path) -> Result<()> {
        match (fs::symlink_metadata(path), &self.replaced) {
            (Err(error), None) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            (Ok(current), Some(previous)) if current.is_file() && unchanged(previous, &current) => {
                Ok(())
            }
            (Err(error), _) if error.kind() != std::io::ErrorKind::NotFound => Err(error.into()),
            _ => anyhow::bail!(
                "Export destination already exists or changed; select it again and confirm replacement"
            ),
        }
    }
}

fn inspect_ancestors(path: &Path) -> Result<Vec<ExportAncestor>> {
    path.parent()
        .context("Export path has no parent directory")?
        .ancestors()
        .map(|path| {
            let identity = match fs::symlink_metadata(path) {
                Ok(metadata) => {
                    ensure!(
                        metadata.is_dir(),
                        "Export directory changed; select the destination again"
                    );
                    Some(metadata)
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
                Err(error) => return Err(error.into()),
            };
            Ok(ExportAncestor {
                path: path.to_path_buf(),
                identity,
            })
        })
        .collect()
}

fn check_ancestors(ancestors: &[ExportAncestor], allow_created: bool) -> Result<()> {
    for ancestor in ancestors {
        let matches = match (&ancestor.identity, fs::symlink_metadata(&ancestor.path)) {
            (Some(before), Ok(after)) if after.is_dir() => {
                #[cfg(unix)]
                {
                    same_file(before, &after)
                }
                #[cfg(not(unix))]
                {
                    before.created().ok() == after.created().ok()
                }
            }
            (None, Err(error)) => error.kind() == std::io::ErrorKind::NotFound,
            (None, Ok(after)) => allow_created && after.is_dir(),
            _ => false,
        };
        ensure!(
            matches,
            "Export directory changed; select the destination again"
        );
    }
    Ok(())
}

fn same_file(left: &Metadata, right: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        left.dev() == right.dev() && left.ino() == right.ino()
    }
    #[cfg(not(unix))]
    {
        let _ = (left, right);
        false
    }
}

fn unchanged(left: &Metadata, right: &Metadata) -> bool {
    let attributes_match = left.len() == right.len()
        && left.modified().ok() == right.modified().ok()
        && left.permissions() == right.permissions();
    #[cfg(unix)]
    {
        attributes_match && same_file(left, right)
    }
    #[cfg(not(unix))]
    {
        attributes_match
    }
}

fn resolve_path(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path)?;
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => continue,
            Component::ParentDir => {
                resolved.pop();
            }
            _ => resolved.push(component.as_os_str()),
        }
        match fs::canonicalize(&resolved) {
            Ok(canonical) => resolved = canonical,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                ensure!(
                    fs::symlink_metadata(&resolved).is_err(),
                    "Export path contains an unresolved symbolic link"
                );
            }
            Err(error) => return Err(error.into()),
        }
    }
    Ok(resolved)
}

impl<D: DataProvider> App<D> {
    pub(super) fn validate_export_target(&self, target: &ExportTarget) -> Result<()> {
        let destination = resolve_path(target.path())?;
        let mut protected = Vec::new();
        let backend = self.settings.backend_type.unwrap_or_default();
        match backend {
            #[cfg(feature = "json")]
            BackendType::Json => {
                let path = self
                    .settings
                    .json_backend
                    .file_path
                    .clone()
                    .map_or_else(crate::settings::json_backend::get_default_json_path, Ok)?;
                let mut lock = path.as_os_str().to_os_string();
                lock.push(".lock");
                protected.push((PathBuf::from(lock), false));
                protected.push((path, false));
            }
            #[cfg(feature = "sqlite")]
            BackendType::Sqlite => {
                let path =
                    self.settings.sqlite_backend.file_path.clone().map_or_else(
                        crate::settings::sqlite_backend::get_default_sqlite_path,
                        Ok,
                    )?;
                for suffix in ["", "-journal", "-wal", "-shm"] {
                    let mut file = path.as_os_str().to_os_string();
                    file.push(suffix);
                    protected.push((PathBuf::from(file), false));
                }
            }
            #[cfg(feature = "file")]
            BackendType::File => protected.push((
                self.settings
                    .file_backend
                    .directory
                    .clone()
                    .map_or_else(crate::settings::file_backend::get_default_file_path, Ok)?,
                true,
            )),
            #[cfg(feature = "vjournal")]
            BackendType::Vjournal => protected.push((
                self.settings
                    .vjournal_backend
                    .directory
                    .clone()
                    .map_or_else(
                        crate::settings::vjournal_backend::get_default_vjournal_path,
                        Ok,
                    )?,
                true,
            )),
            #[allow(unreachable_patterns)]
            _ => {}
        }
        if let Some(context) = &self.config_context {
            protected.push((context.settings_path.clone(), false));
            protected.push((context.themes_path.clone(), true));
            if let Some(parent) = context.themes_path.parent() {
                protected.push((parent.join("themes.toml"), false));
            }
        } else {
            protected.push((Settings::file_path(None)?, false));
            protected.push((settings_default_dir_path()?.join("themes"), true));
            protected.push((settings_default_dir_path()?.join("themes.toml"), false));
        }
        let state_directory = self
            .settings
            .app_state_dir
            .clone()
            .map_or_else(AppState::default_persist_dir, Ok)?;
        protected.push((state_directory.join("state.json"), false));

        for (path, directory) in protected {
            let canonical = resolve_path(&path)?;
            let alias = match (fs::metadata(&destination), fs::metadata(&canonical)) {
                (Ok(left), Ok(right)) => same_file(&left, &right),
                _ => false,
            };
            ensure!(
                destination != canonical
                    && !(directory && destination.starts_with(&canonical))
                    && !alias,
                "Choose an export destination outside the app's active journal, configuration, themes, and state files"
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;

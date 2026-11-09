use clap::Parser;
#[cfg(any(feature = "json", feature = "sqlite", test))]
use std::path::Path;
use std::{fs, path::PathBuf};

use crate::{
    logging::{get_default_path as default_log_path, setup_logging},
    settings::{BackendType, Settings, settings_default_dir_path},
};

pub mod backup;
pub mod commands;
pub use commands::CliCommand;
pub use commands::PendingCliCommand;
#[cfg(any(feature = "json", feature = "sqlite", feature = "vjournal"))]
use path_absolutize::Absolutize;

#[derive(Parser, Debug)]
#[command(author, version, about)]
pub struct Cli {
    /// Sets the entries Json file path and starts using it.
    #[arg(short, long, value_name = "FILE PATH")]
    #[cfg(feature = "json")]
    json_file_path: Option<PathBuf>,

    /// Sets the entries sqlite file path and starts using it.
    #[arg(short, long, value_name = "FILE PATH")]
    #[cfg(feature = "sqlite")]
    sqlite_file_path: Option<PathBuf>,

    /// Sets the vjournal directory path and starts using it.
    #[arg(long = "vjournal-dir-path", value_name = "DIR PATH")]
    #[cfg(feature = "vjournal")]
    vjournal_dir_path: Option<PathBuf>,

    /// Sets the plain-text/Markdown journal directory path and starts using it.
    #[arg(long = "file-dir-path", value_name = "DIR PATH")]
    #[cfg(feature = "file")]
    file_dir_path: Option<PathBuf>,

    /// Sets the backend type and starts using it.
    #[arg(short, long, value_enum)]
    backend_type: Option<BackendType>,

    #[arg(short = 'c', long = "config", value_name = "DIR PATH", help = config_help())]
    pub config_path: Option<PathBuf>,

    /// Increases logging verbosity each use for up to 3 times.
    #[arg(short = 'v', long, action = clap::ArgAction::Count)]
    verbose: u8,

    #[arg(short = 'l', long = "log", value_name = "FILE PATH", help = log_help())]
    log_file: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<CliCommand>,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum CliResult {
    Return,
    Continue,
    PendingCommand(PendingCliCommand),
}

impl Cli {
    pub fn shows_journal_selector(&self) -> bool {
        if self.command.is_some() || self.backend_type.is_some() {
            return false;
        }
        #[cfg(feature = "json")]
        if self.json_file_path.is_some() {
            return false;
        }
        #[cfg(feature = "sqlite")]
        if self.sqlite_file_path.is_some() {
            return false;
        }
        #[cfg(feature = "vjournal")]
        if self.vjournal_dir_path.is_some() {
            return false;
        }
        #[cfg(feature = "file")]
        if self.file_dir_path.is_some() {
            return false;
        }
        true
    }

    /// Resolve the effective CLI storage before any directories are created, so
    /// a managed journal can be leased before a path override opens its files.
    pub fn storage_override_settings(&self, base: &Settings) -> anyhow::Result<Option<Settings>> {
        let mut settings = base.clone();
        let mut overridden = false;
        #[cfg(feature = "json")]
        if let Some(path) = &self.json_file_path {
            settings.json_backend.file_path = Some(path.absolutize()?.into_owned());
            settings.backend_type = Some(BackendType::Json);
            overridden = true;
        }
        #[cfg(feature = "sqlite")]
        if let Some(path) = &self.sqlite_file_path {
            settings.sqlite_backend.file_path = Some(path.absolutize()?.into_owned());
            settings.backend_type = Some(BackendType::Sqlite);
            overridden = true;
        }
        #[cfg(feature = "vjournal")]
        if let Some(path) = &self.vjournal_dir_path {
            settings.vjournal_backend.directory = Some(path.absolutize()?.into_owned());
            settings.backend_type = Some(BackendType::Vjournal);
            overridden = true;
        }
        #[cfg(feature = "file")]
        if let Some(path) = &self.file_dir_path {
            settings.file_backend.directory = Some(std::path::absolute(path)?);
            settings.backend_type = Some(BackendType::File);
            overridden = true;
        }
        if let Some(backend) = self.backend_type {
            set_backend_type(backend, &mut settings);
            overridden = true;
        }
        Ok(overridden.then_some(settings))
    }

    pub async fn execute_backup(&self) -> anyhow::Result<bool> {
        let Some(CliCommand::Backup(command)) = &self.command else {
            return Ok(false);
        };
        match command {
            backup::BackupCommand::Restore { directory, to } => {
                backup::restore(directory, to)?;
                println!(
                    "Restored a separate profile at {}. Open it with --config pointing to its config directory.",
                    std::path::absolute(to)?.display()
                );
            }
            backup::BackupCommand::Create { directory } => {
                let config_path = Settings::file_path(self.config_path.clone())?;
                let journal = crate::journals::journal_for_config(&config_path)?;
                let _journal_lease = journal
                    .as_ref()
                    .map(|(store, journal)| store.acquire(journal))
                    .transpose()?;
                let settings = Settings::new(self.config_path.clone()).await?;
                let settings = self
                    .storage_override_settings(&settings)?
                    .unwrap_or(settings);
                let _storage_lease = crate::journals::journal_for_storage(&settings)?
                    .filter(|(_, owner)| {
                        journal
                            .as_ref()
                            .is_none_or(|(_, active)| active.settings_path != owner.settings_path)
                    })
                    .map(|(store, owner)| store.acquire(&owner))
                    .transpose()?;
                backup::create(&settings, self.config_path.as_ref(), directory).await?;
                println!(
                    "Backup written to {}",
                    std::path::absolute(directory)?.display()
                );
            }
        }
        Ok(true)
    }

    pub fn launch_details(&self) -> anyhow::Result<Vec<String>> {
        let mut details = vec![
            format!(
                "Working directory: {} (relative paths resolve here)",
                std::env::current_dir()?.display()
            ),
            format!(
                "Log: {}",
                std::path::absolute(self.log_file.clone().map_or_else(default_log_path, Ok)?)?
                    .display()
            ),
            format!("Logging verbosity: {} (launch option)", self.verbose),
        ];
        #[cfg(feature = "json")]
        if let Some(path) = &self.json_file_path {
            details.push(format!("CLI override: JSON file = {}", path.display()));
        }
        #[cfg(feature = "sqlite")]
        if let Some(path) = &self.sqlite_file_path {
            details.push(format!("CLI override: SQLite file = {}", path.display()));
        }
        #[cfg(feature = "vjournal")]
        if let Some(path) = &self.vjournal_dir_path {
            details.push(format!(
                "CLI override: VJOURNAL directory = {}",
                path.display()
            ));
        }
        #[cfg(feature = "file")]
        if let Some(path) = &self.file_dir_path {
            details.push(format!("CLI override: File directory = {}", path.display()));
        }
        if let Some(backend) = &self.backend_type {
            details.push(format!("CLI override: backend = {backend:?}"));
        }
        Ok(details)
    }

    pub async fn handle_cli(mut self, settings: &mut Settings) -> anyhow::Result<CliResult> {
        if let Some(overrides) = self.storage_override_settings(settings)? {
            *settings = overrides;
        }
        // Only the selected backend needs directories. Inactive overrides must
        // not recreate an archived journal or touch another journal's storage.
        match settings.backend_type.unwrap_or_default() {
            #[cfg(feature = "json")]
            BackendType::Json => {
                if let Some(path) = self.json_file_path.take() {
                    set_json_path(path, settings).await?;
                }
            }
            #[cfg(feature = "sqlite")]
            BackendType::Sqlite => {
                if let Some(path) = self.sqlite_file_path.take() {
                    set_sqlite_path(path, settings).await?;
                }
            }
            #[cfg(feature = "vjournal")]
            BackendType::Vjournal => {
                if let Some(path) = self.vjournal_dir_path.take() {
                    set_vjournal_path(path, settings).await?;
                }
            }
            #[cfg(feature = "file")]
            BackendType::File => {
                if let Some(path) = self.file_dir_path.take() {
                    set_file_path(path, settings)?;
                }
            }
            #[allow(unreachable_patterns)]
            _ => {}
        }

        let log_path = self.log_file.take().map_or_else(default_log_path, Ok)?;
        crate::logging::validate_destination(&log_path, settings, self.config_path.clone())?;
        setup_logging(self.verbose, Some(log_path))?;

        if let Some(cmd) = self.command.take() {
            cmd.exec(settings, self.config_path.as_ref())
        } else {
            Ok(CliResult::Continue)
        }
    }
}

#[cfg(feature = "json")]
async fn set_json_path(path: PathBuf, settings: &mut Settings) -> anyhow::Result<()> {
    ensure_path_exists(&path).await?;

    settings.json_backend.file_path = path.absolutize().map(PathBuf::from).ok();

    Ok(())
}

#[cfg(any(feature = "json", feature = "sqlite", test))]
async fn ensure_path_exists(path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        return Ok(());
    }

    if let Some(parent) = path.parent() {
        create_private_dirs(parent)?;
    }

    Ok(())
}

#[cfg(feature = "sqlite")]
async fn set_sqlite_path(path: PathBuf, settings: &mut Settings) -> anyhow::Result<()> {
    ensure_path_exists(&path).await?;

    settings.sqlite_backend.file_path = path.absolutize().map(PathBuf::from).ok();

    Ok(())
}

#[cfg(feature = "vjournal")]
async fn set_vjournal_path(path: PathBuf, settings: &mut Settings) -> anyhow::Result<()> {
    if !path.exists() {
        create_private_dirs(&path)?;
    }

    settings.vjournal_backend.directory = path.absolutize().map(PathBuf::from).ok();

    Ok(())
}

#[cfg(feature = "file")]
fn set_file_path(path: PathBuf, settings: &mut Settings) -> anyhow::Result<()> {
    create_private_dirs(&path)?;
    settings.file_backend.directory = Some(std::path::absolute(path)?);
    Ok(())
}

#[inline]
fn set_backend_type(backend: BackendType, settings: &mut Settings) {
    settings.backend_type = Some(backend);
}

fn create_private_dirs(path: &std::path::Path) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder.create(path)
}

fn log_help() -> String {
    format!(
        "Specifies a file to use for logging\n(default file: {})",
        default_log_path()
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_default()
    )
}

fn config_help() -> String {
    format!(
        "Specifies the path for the configuration directory.\n\
            The configuration directory is considered as the root for themes files too.\n\
            It still accepts the path for configuration file for backward compatibility.\n\
            (default path: {})",
        settings_default_dir_path()
            .map(|path| path.to_string_lossy().to_string())
            .unwrap_or_default()
    )
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use clap::Parser;
    use tempfile::Builder;

    use super::*;

    #[test]
    fn selector_is_for_normal_launches_and_preserves_explicit_cli_destinations() {
        for args in [
            vec!["mjournal"],
            vec!["mjournal", "--config", "preview", "--log", "preview.log"],
        ] {
            assert!(Cli::parse_from(args).shows_journal_selector());
        }
        for args in [
            vec!["mjournal", "print-config"],
            vec!["mjournal", "licenses"],
            vec!["mjournal", "theme", "print-default"],
            vec!["mjournal", "import-journals", "--path", "import.json"],
            vec!["mjournal", "assign-priority", "3"],
            vec!["mjournal", "--backend-type", "json"],
        ] {
            assert!(!Cli::parse_from(args).shows_journal_selector());
        }
        #[cfg(feature = "json")]
        assert!(
            !Cli::parse_from(["mjournal", "--json-file-path", "entries.json"])
                .shows_journal_selector()
        );
        #[cfg(feature = "sqlite")]
        assert!(
            !Cli::parse_from(["mjournal", "--sqlite-file-path", "entries.db"])
                .shows_journal_selector()
        );
        #[cfg(feature = "file")]
        assert!(
            !Cli::parse_from(["mjournal", "--file-dir-path", "entries"]).shows_journal_selector()
        );
        #[cfg(feature = "vjournal")]
        assert!(
            !Cli::parse_from(["mjournal", "--vjournal-dir-path", "entries"])
                .shows_journal_selector()
        );
    }

    #[test]
    fn storage_override_preflight_does_not_create_files_or_change_base_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing/storage");
        let path_text = path.to_str().unwrap();
        let base = Settings::default();
        assert!(
            Cli::parse_from(["mjournal"])
                .storage_override_settings(&base)
                .unwrap()
                .is_none()
        );
        let overrides = [
            #[cfg(feature = "json")]
            ("--json-file-path", BackendType::Json),
            #[cfg(feature = "sqlite")]
            ("--sqlite-file-path", BackendType::Sqlite),
            #[cfg(feature = "vjournal")]
            ("--vjournal-dir-path", BackendType::Vjournal),
            #[cfg(feature = "file")]
            ("--file-dir-path", BackendType::File),
        ];
        for (flag, backend) in overrides {
            let settings = Cli::parse_from(["mjournal", flag, path_text])
                .storage_override_settings(&base)
                .unwrap()
                .unwrap();
            assert_eq!(settings.backend_type, Some(backend));
            match backend {
                #[cfg(feature = "json")]
                BackendType::Json => {
                    assert_eq!(settings.json_backend.file_path.as_ref(), Some(&path))
                }
                #[cfg(feature = "sqlite")]
                BackendType::Sqlite => {
                    assert_eq!(settings.sqlite_backend.file_path.as_ref(), Some(&path))
                }
                #[cfg(feature = "vjournal")]
                BackendType::Vjournal => {
                    assert_eq!(settings.vjournal_backend.directory.as_ref(), Some(&path))
                }
                #[cfg(feature = "file")]
                BackendType::File => {
                    assert_eq!(settings.file_backend.directory.as_ref(), Some(&path))
                }
                #[allow(unreachable_patterns)]
                _ => unreachable!(),
            }
            assert!(!path.parent().unwrap().exists());
        }
        assert_eq!(base.backend_type, None);
    }

    #[cfg(all(feature = "json", feature = "file"))]
    #[test]
    fn storage_preflight_matches_cli_backend_override_precedence() {
        let cli = Cli::parse_from([
            "mjournal",
            "--json-file-path",
            "json.json",
            "--file-dir-path",
            "files",
            "--backend-type",
            "json",
        ]);
        let settings = cli
            .storage_override_settings(&Settings::default())
            .unwrap()
            .unwrap();
        assert_eq!(settings.backend_type, Some(BackendType::Json));
        assert_eq!(
            settings.json_backend.file_path,
            Some(std::path::absolute("json.json").unwrap())
        );
        assert_eq!(
            settings.file_backend.directory,
            Some(std::path::absolute("files").unwrap())
        );
    }

    #[cfg(unix)]
    #[test]
    fn cli_creates_private_directories_without_changing_existing_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let existing = dir.path().join("existing");
        fs::create_dir(&existing).unwrap();
        fs::set_permissions(&existing, fs::Permissions::from_mode(0o750)).unwrap();
        let destination = existing.join("new/nested");
        create_private_dirs(&destination).unwrap();
        assert_eq!(
            fs::metadata(&existing).unwrap().permissions().mode() & 0o777,
            0o750
        );
        assert_eq!(
            fs::metadata(destination.parent().unwrap())
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(destination).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }

    #[test]
    fn parse_backend_and_config() {
        let cli = Cli::parse_from([
            "mjournal",
            "--backend-type",
            "json",
            "--config",
            "/tmp/config",
            "-vv",
            "assign-priority",
            "4",
        ]);

        assert_eq!(cli.backend_type, Some(BackendType::Json));
        assert_eq!(cli.config_path, Some(PathBuf::from("/tmp/config")));
        assert_eq!(cli.verbose, 2);
        assert_eq!(
            cli.command,
            Some(CliCommand::AssignPriority { priority: 4 })
        );
    }

    #[test]
    fn parse_rejects_bad_backend() {
        let err = Cli::try_parse_from(["mjournal", "--backend-type", "bogus"]).unwrap_err();

        assert!(err.to_string().contains("invalid value"));
    }

    #[tokio::test]
    async fn handle_cli_without_command() {
        let dir = Builder::new().prefix("cli-log").tempdir().unwrap();
        let mut settings = Settings::default();
        let cli = Cli::parse_from([
            "mjournal",
            "--log",
            dir.path().join("journal.log").to_str().unwrap(),
        ]);
        #[cfg(feature = "file")]
        let cli = Cli {
            file_dir_path: Some(dir.path().join("file-entries")),
            ..cli
        };

        let result = cli.handle_cli(&mut settings).await.unwrap();

        assert_eq!(result, CliResult::Continue);
        #[cfg(feature = "file")]
        {
            assert_eq!(settings.backend_type, Some(BackendType::File));
            assert_eq!(
                settings.file_backend.directory,
                Some(dir.path().join("file-entries"))
            );
            assert!(dir.path().join("file-entries").is_dir());
        }
    }

    #[tokio::test]
    async fn ensure_path_creates_parent() {
        let dir = Builder::new().prefix("cli-parent").tempdir().unwrap();
        let file_path = dir.path().join("nested").join("entries.json");

        ensure_path_exists(&file_path).await.unwrap();

        assert!(file_path.parent().unwrap().exists());
    }

    #[cfg(feature = "json")]
    #[tokio::test]
    async fn set_json_path_absolutizes() {
        let dir = Builder::new().prefix("cli-json").tempdir().unwrap();
        let mut settings = Settings::default();
        let file_path = dir.path().join("entries.json");

        set_json_path(file_path.clone(), &mut settings)
            .await
            .unwrap();

        assert_eq!(
            settings.json_backend.file_path,
            Some(file_path.absolutize().unwrap().into_owned())
        );
    }

    #[cfg(feature = "sqlite")]
    #[tokio::test]
    async fn set_sqlite_path_absolutizes() {
        let dir = Builder::new().prefix("cli-sqlite").tempdir().unwrap();
        let mut settings = Settings::default();
        let file_path = dir.path().join("entries.db");

        set_sqlite_path(file_path.clone(), &mut settings)
            .await
            .unwrap();

        assert_eq!(
            settings.sqlite_backend.file_path,
            Some(file_path.absolutize().unwrap().into_owned())
        );
    }

    #[cfg(feature = "vjournal")]
    #[tokio::test]
    async fn set_vjournal_path_absolutizes() {
        let dir = Builder::new().prefix("cli-vjournal").tempdir().unwrap();
        let mut settings = Settings::default();
        let dir_path = dir.path().join("journal");

        set_vjournal_path(dir_path.clone(), &mut settings)
            .await
            .unwrap();

        assert!(dir_path.exists());
        assert_eq!(
            settings.vjournal_backend.directory,
            Some(dir_path.absolutize().unwrap().into_owned())
        );
    }

    #[test]
    fn backend_type_sets_value() {
        let mut settings = Settings::default();

        set_backend_type(BackendType::default(), &mut settings);

        assert_eq!(settings.backend_type, Some(BackendType::default()));
    }

    #[cfg(feature = "file")]
    #[test]
    fn file_cli_path_and_explicit_backend_are_visible_as_launch_overrides() {
        let cli = Cli::parse_from([
            "mjournal",
            "--file-dir-path",
            "notes",
            "--backend-type",
            "file",
        ]);
        assert_eq!(cli.file_dir_path, Some(PathBuf::from("notes")));
        assert_eq!(cli.backend_type, Some(BackendType::File));
        let details = cli.launch_details().unwrap();
        assert!(details.contains(&"CLI override: File directory = notes".to_string()));
        assert!(details.contains(&"CLI override: backend = File".to_string()));
    }

    #[cfg(feature = "file")]
    #[test]
    fn file_directory_rejects_regular_files_without_changing_settings() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("existing-file");
        fs::write(&file, "keep").unwrap();
        let mut settings = Settings::default();
        assert!(set_file_path(file.clone(), &mut settings).is_err());
        assert_eq!(settings.file_backend.directory, None);
        assert_eq!(fs::read_to_string(file).unwrap(), "keep");
    }

    #[cfg(all(feature = "file", unix))]
    #[test]
    fn file_directory_keeps_parent_components_after_symlinks() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join("real/child")).unwrap();
        std::os::unix::fs::symlink(dir.path().join("real/child"), dir.path().join("linked"))
            .unwrap();
        let mut settings = Settings::default();
        set_file_path(dir.path().join("linked/../notes"), &mut settings).unwrap();
        assert_eq!(
            fs::canonicalize(settings.file_backend.directory.unwrap()).unwrap(),
            fs::canonicalize(dir.path().join("real/notes")).unwrap()
        );
        assert!(!dir.path().join("notes").exists());
    }
}

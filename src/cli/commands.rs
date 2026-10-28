use std::path::PathBuf;

use clap::Subcommand;

use crate::{
    app::ui::{Styles, themes::ThemeStore},
    settings::Settings,
};

use super::*;

#[derive(Subcommand, Debug, Clone, PartialEq, Eq)]
pub enum CliCommand {
    /// Create local backups or restore them into a separate profile.
    #[command(subcommand)]
    Backup(super::backup::BackupCommand),
    /// Print the app license, attribution, and third-party notices without opening any data files.
    Licenses,
    /// Print the current settings including the paths for the back-end files.
    #[clap(visible_alias = "pc")]
    PrintConfig,
    /// Import journals from the given transfer JSON file to the current back-end file.
    #[clap(visible_alias = "imj")]
    ImportJournals {
        /// Path of the JSON file to import from.
        #[arg(short = 'p', long = "path", required = true, value_name = "FILE PATH")]
        file_path: PathBuf,
    },
    /// Assign priority for all the entries with empty priority field
    #[clap(visible_alias = "ap")]
    AssignPriority {
        /// Priority value (Positive number)
        #[arg(required = true, value_name = "PRIORITY", index = 1)]
        priority: u32,
    },
    /// Provides commands regarding changing themes and styles of the app.
    #[clap(visible_alias = "style")]
    #[command(subcommand)]
    Theme(Themes),
}

#[derive(Debug, Clone, Subcommand, Eq, PartialEq)]
pub enum Themes {
    #[clap(visible_alias = "path")]
    /// Prints the path to the named themes directory without creating it.
    PrintPath,
    #[clap(name = "print-default", visible_alias = "default")]
    /// Dumps the styles with the default values to be used as a reference and base for
    /// user custom themes.
    DumpDefaults,
    #[command(name = "write-new", visible_alias = "write", disable_help_flag = true)]
    /// Creates a new theme with the original Tui defaults; all following words form its name.
    WriteNew {
        /// Theme name; leave empty for tui_custom, tui_custom_01, tui_custom_02, and so on.
        #[arg(value_name = "NAME", num_args = 0.., trailing_var_arg = true, allow_hyphen_values = true)]
        name: Vec<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PendingCliCommand {
    ImportJournals(PathBuf),
    AssignPriority(u32),
}

impl CliCommand {
    pub fn exec(
        self,
        settings: &mut Settings,
        custom_config_dir: Option<&PathBuf>,
    ) -> anyhow::Result<CliResult> {
        match self {
            CliCommand::Backup(_) => {
                anyhow::bail!("Backup commands must run before application startup")
            }
            CliCommand::Licenses => {
                print_licenses()?;
                Ok(CliResult::Return)
            }
            CliCommand::PrintConfig => exec_print_config(settings),
            CliCommand::ImportJournals { file_path: path } => Ok(CliResult::PendingCommand(
                PendingCliCommand::ImportJournals(path),
            )),
            CliCommand::AssignPriority { priority } => Ok(CliResult::PendingCommand(
                PendingCliCommand::AssignPriority(priority),
            )),
            CliCommand::Theme(cmd) => match cmd {
                Themes::PrintPath => exec_print_themes_path(custom_config_dir),
                Themes::DumpDefaults => exec_print_themes_defaults(),
                Themes::WriteNew { name } => {
                    exec_write_new_theme(custom_config_dir, &name.join(" "))
                }
            },
        }
    }
}

pub fn print_licenses() -> anyhow::Result<()> {
    use std::io::Write;

    let mut output = std::io::stdout().lock();
    output.write_all(
        concat!(
            "MynaJournal license\n\n",
            include_str!("../../LICENSE"),
            "\n\n",
            include_str!("../../ATTRIBUTION.md"),
            "\n\n",
            include_str!("../../THIRD_PARTY_NOTICES.txt"),
        )
        .as_bytes(),
    )?;
    Ok(())
}

fn exec_print_config(settings: &mut Settings) -> anyhow::Result<CliResult> {
    let settings_text = settings.get_as_text()?;

    println!("{settings_text}");

    Ok(CliResult::Return)
}

fn exec_print_themes_path(custom_config_dir: Option<&PathBuf>) -> anyhow::Result<CliResult> {
    let store = ThemeStore::new(custom_config_dir)?;
    let themes_path = store.directory();

    println!("{}", themes_path.display());

    Ok(CliResult::Return)
}

fn exec_print_themes_defaults() -> anyhow::Result<CliResult> {
    let themes_txt = Styles::serialize_default()?;
    println!("{themes_txt}");

    Ok(CliResult::Return)
}

fn exec_write_new_theme(
    custom_config_dir: Option<&PathBuf>,
    name: &str,
) -> anyhow::Result<CliResult> {
    let themes_path = ThemeStore::new(custom_config_dir)?.write_new(name)?;

    println!(
        "New theme with the original Tui defaults has been written to {}",
        themes_path.display()
    );

    Ok(CliResult::Return)
}

#[cfg(test)]
mod tests {
    use std::fs;

    use clap::Parser;

    use super::*;
    use crate::cli::Cli;

    #[test]
    fn licenses_command_parses() {
        assert_eq!(
            Cli::parse_from(["mjournal", "licenses"]).command,
            Some(CliCommand::Licenses)
        );
    }

    #[test]
    fn aliases_parse() {
        let print = Cli::parse_from(["mjournal", "pc"]);
        let import = Cli::parse_from(["mjournal", "imj", "--path", "/tmp/in.json"]);
        let priority = Cli::parse_from(["mjournal", "ap", "7"]);
        let theme = Cli::parse_from(["mjournal", "style", "path"]);

        assert_eq!(print.command, Some(CliCommand::PrintConfig));
        assert_eq!(
            import.command,
            Some(CliCommand::ImportJournals {
                file_path: PathBuf::from("/tmp/in.json"),
            })
        );
        assert_eq!(
            priority.command,
            Some(CliCommand::AssignPriority { priority: 7 })
        );
        assert_eq!(theme.command, Some(CliCommand::Theme(Themes::PrintPath)));
    }

    #[test]
    fn import_exec_returns_pending() {
        let mut settings = Settings::default();

        let result = CliCommand::ImportJournals {
            file_path: PathBuf::from("/tmp/import.json"),
        }
        .exec(&mut settings, None)
        .unwrap();

        assert_eq!(
            result,
            CliResult::PendingCommand(PendingCliCommand::ImportJournals(PathBuf::from(
                "/tmp/import.json"
            )))
        );
    }

    #[test]
    fn assign_exec_returns_pending() {
        let mut settings = Settings::default();

        let result = CliCommand::AssignPriority { priority: 5 }
            .exec(&mut settings, None)
            .unwrap();

        assert_eq!(
            result,
            CliResult::PendingCommand(PendingCliCommand::AssignPriority(5))
        );
    }

    #[test]
    fn theme_commands_return() {
        let mut settings = Settings::default();
        let dir = tempfile::Builder::new()
            .prefix("themes-return")
            .tempdir()
            .unwrap();

        let print_path = CliCommand::Theme(Themes::PrintPath)
            .exec(&mut settings, Some(&dir.path().to_path_buf()))
            .unwrap();
        let dump_defaults = CliCommand::Theme(Themes::DumpDefaults)
            .exec(&mut settings, Some(&dir.path().to_path_buf()))
            .unwrap();

        assert_eq!(print_path, CliResult::Return);
        assert_eq!(dump_defaults, CliResult::Return);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 0);
    }

    #[test]
    fn write_new_parses_all_following_words_as_the_name() {
        for args in [
            vec!["Night", "Blue"],
            vec!["Night Blue"],
            vec!["--help"],
            vec!["-Night", "--Blue"],
            vec!["Night", "--config", "Blue"],
        ] {
            let mut command = vec!["mjournal", "theme", "write-new"];
            command.extend(&args);
            let cli = Cli::try_parse_from(command).unwrap();
            assert_eq!(
                cli.command,
                Some(CliCommand::Theme(Themes::WriteNew {
                    name: args.into_iter().map(String::from).collect(),
                }))
            );
        }
    }

    #[test]
    fn write_new_replaces_write_defaults_and_keeps_the_write_alias() {
        assert!(Cli::try_parse_from(["mjournal", "theme", "write-defaults"]).is_err());
        assert_eq!(
            Cli::parse_from(["mjournal", "style", "write"]).command,
            Some(CliCommand::Theme(Themes::WriteNew { name: vec![] }))
        );
        assert_eq!(
            Cli::try_parse_from(["mjournal", "theme", "--help"])
                .unwrap_err()
                .kind(),
            clap::error::ErrorKind::DisplayHelp
        );
    }

    #[test]
    fn write_new_fails_if_named_theme_exists() {
        let mut settings = Settings::default();
        let dir = tempfile::Builder::new()
            .prefix("themes-exists")
            .tempdir()
            .unwrap();
        fs::create_dir(dir.path().join("themes")).unwrap();
        let existing = dir.path().join("themes/theme_night_blue.toml");
        fs::write(&existing, "already here").unwrap();

        let err = CliCommand::Theme(Themes::WriteNew {
            name: vec!["Night".into(), "Blue".into()],
        })
        .exec(&mut settings, Some(&dir.path().to_path_buf()))
        .unwrap_err();

        assert!(err.to_string().contains("Theme already exists"));
        assert_eq!(fs::read_to_string(existing).unwrap(), "already here");
    }

    #[test]
    fn write_new_uses_original_defaults_without_changing_configuration_or_initializing_the_catalog()
    {
        let mut settings = Settings {
            theme: Some("night".into()),
            ..Settings::default()
        };
        let directory = tempfile::tempdir().unwrap();
        let config_path = directory.path().join("config.toml");
        let config = "# Active selection must remain unchanged\ntheme = 'night'\n";
        fs::write(&config_path, config).unwrap();
        fs::create_dir(directory.path().join("themes")).unwrap();
        let tui_path = directory.path().join("themes/theme_tui.toml");
        let customized_tui = "[general.input_block_active]\nfg = 'Red'\n";
        fs::write(&tui_path, customized_tui).unwrap();
        let result = CliCommand::Theme(Themes::WriteNew {
            name: vec!["Night".into(), "Blue".into()],
        })
        .exec(&mut settings, Some(&directory.path().to_path_buf()))
        .unwrap();

        assert_eq!(result, CliResult::Return);
        let text =
            fs::read_to_string(directory.path().join("themes/theme_night_blue.toml")).unwrap();
        let created: toml::Value = toml::from_str(&text).unwrap();
        let defaults: toml::Value = toml::from_str(&Styles::serialize_default().unwrap()).unwrap();
        for (key, value) in defaults.as_table().unwrap() {
            assert_eq!(&created[key], value);
        }
        assert_eq!(created["theme"]["name"].as_str(), Some("Night Blue"));
        assert_eq!(fs::read_to_string(tui_path).unwrap(), customized_tui);
        assert_eq!(fs::read_to_string(config_path).unwrap(), config);
        assert_eq!(settings.theme.as_deref(), Some("night"));
        assert_eq!(
            fs::read_dir(directory.path().join("themes"))
                .unwrap()
                .count(),
            2
        );
    }

    #[test]
    fn write_new_without_a_name_uses_the_custom_filename_sequence() {
        let mut settings = Settings::default();
        let directory = tempfile::tempdir().unwrap();
        for filename in [
            "theme_tui_custom.toml",
            "theme_tui_custom_01.toml",
            "theme_tui_custom_02.toml",
        ] {
            let cli = Cli::parse_from(["mjournal", "theme", "write-new"]);
            assert_eq!(
                cli.command
                    .unwrap()
                    .exec(&mut settings, Some(&directory.path().to_path_buf()))
                    .unwrap(),
                CliResult::Return
            );
            assert!(directory.path().join("themes").join(filename).is_file());
        }
        assert!(!directory.path().join("config.toml").exists());
        assert_eq!(
            fs::read_dir(directory.path().join("themes"))
                .unwrap()
                .count(),
            3
        );
    }

    #[test]
    fn write_new_rejects_invalid_names_without_creating_files() {
        let mut settings = Settings::default();
        let directory = tempfile::tempdir().unwrap();
        for name in ["../escape", "a/b", "Tui", "Legacy", "."] {
            assert!(
                CliCommand::Theme(Themes::WriteNew {
                    name: vec![name.into()]
                })
                .exec(&mut settings, Some(&directory.path().to_path_buf()))
                .is_err()
            );
        }
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[test]
    fn write_new_accepts_former_bundled_theme_names() {
        let mut settings = Settings::default();
        let directory = tempfile::tempdir().unwrap();
        for (name, id) in [
            ("Dark Abyssal", "dark_abyssal"),
            ("Light Sorbet", "light_sorbet"),
        ] {
            let result = CliCommand::Theme(Themes::WriteNew {
                name: vec![name.into()],
            })
            .exec(&mut settings, Some(&directory.path().to_path_buf()))
            .unwrap();
            assert_eq!(result, CliResult::Return);
            let text = fs::read_to_string(directory.path().join(format!("themes/theme_{id}.toml")))
                .unwrap();
            assert_eq!(
                toml::Value::try_from(Styles::deserialize(&text).unwrap()).unwrap(),
                toml::Value::try_from(Styles::default()).unwrap()
            );
        }
        assert_eq!(
            fs::read_dir(directory.path().join("themes"))
                .unwrap()
                .count(),
            2
        );
        assert!(!directory.path().join("themes/theme_tui.toml").exists());
    }

    #[test]
    fn write_new_reports_an_unusable_themes_directory() {
        let mut settings = Settings::default();
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("themes");
        fs::write(&path, "preserve this file").unwrap();
        assert!(
            CliCommand::Theme(Themes::WriteNew { name: vec![] })
                .exec(&mut settings, Some(&directory.path().to_path_buf()))
                .is_err()
        );
        assert_eq!(fs::read_to_string(path).unwrap(), "preserve this file");
    }
}

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};

use crate::settings::{document::ConfigDocument, settings_default_dir_path};

use super::Styles;

#[derive(Clone, Debug)]
pub struct ThemeStore {
    directory: PathBuf,
    legacy_path: PathBuf,
}

#[derive(Clone, Debug)]
pub struct ThemeInfo {
    pub id: String,
    pub label: String,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct LoadedTheme {
    pub id: Option<String>,
    pub styles: Styles,
    pub warning: Option<String>,
}

#[derive(Clone, Debug)]
pub struct ThemeDocument {
    id: String,
    label: String,
    document: ConfigDocument,
    legacy_source: Option<ConfigDocument>,
    baseline_text: String,
    new_theme: bool,
    store: ThemeStore,
}

impl ThemeStore {
    pub fn new(custom_config_dir: Option<&PathBuf>) -> Result<Self> {
        let config_dir = match custom_config_dir {
            Some(path) if path.is_dir() => path.clone(),
            Some(_) => {
                log::warn!("A legacy --config file path does not change the themes directory");
                settings_default_dir_path()?
            }
            None => settings_default_dir_path()?,
        };
        Self::from_directory(config_dir.join("themes"))
    }

    pub fn from_directory(directory: PathBuf) -> Result<Self> {
        let directory =
            std::path::absolute(directory).context("Resolving themes directory failed")?;
        let legacy_path = directory
            .parent()
            .context("Themes directory has no parent")?
            .join("themes.toml");
        Ok(Self {
            directory,
            legacy_path,
        })
    }

    pub fn directory(&self) -> &Path {
        &self.directory
    }

    pub fn default_id(&self) -> &str {
        if path_present(&self.legacy_path).unwrap_or(true)
            || path_present(&self.directory.join("theme_legacy.toml")).unwrap_or(true)
            || self.files().is_ok_and(|files| files.contains_key("legacy"))
        {
            "legacy"
        } else {
            "tui"
        }
    }

    pub fn resolve(&self, selected: Option<&str>) -> LoadedTheme {
        let id = selected.unwrap_or_else(|| self.default_id());
        match self
            .load(id)
            .and_then(|document| Styles::deserialize(&document.text()))
        {
            Ok(styles) => LoadedTheme {
                id: Some(id.to_owned()),
                styles,
                warning: None,
            },
            Err(error) => LoadedTheme {
                id: None,
                styles: Styles::default(),
                warning: Some(format!(
                    "Could not load theme {id:?}; using built-in Tui defaults: {error:#}"
                )),
            },
        }
    }

    pub fn list(&self) -> Result<Vec<ThemeInfo>> {
        let mut files = self.files()?;
        files.entry("tui".into()).or_default();
        if path_present(&self.legacy_path).unwrap_or(true) {
            files.entry("legacy".into()).or_default();
        }
        let mut themes: Vec<_> = files
            .into_keys()
            .map(|id| match self.load(&id) {
                Ok(theme) => ThemeInfo {
                    id,
                    label: theme.label().to_owned(),
                    error: None,
                },
                Err(error) => ThemeInfo {
                    label: label_for_id(&id),
                    id,
                    error: Some(format!("{error:#}")),
                },
            })
            .collect();
        themes.sort_by(|left, right| {
            (left.id != "tui", left.label.to_ascii_lowercase(), &left.id).cmp(&(
                right.id != "tui",
                right.label.to_ascii_lowercase(),
                &right.id,
            ))
        });
        Ok(themes)
    }

    pub fn load(&self, id: &str) -> Result<ThemeDocument> {
        validate_id(id)?;
        let mut paths = self.files()?.remove(id).unwrap_or_default();
        ensure!(
            paths.len() <= 1,
            "Multiple theme files have the same name: {id}"
        );
        let named_path = paths.pop();
        let mut legacy_source = None;
        let path = named_path
            .clone()
            .unwrap_or_else(|| self.directory.join(format!("theme_{id}.toml")));
        let mut document = ConfigDocument::load(path)?;
        if named_path.is_some() {
            ensure!(
                path_present(document.path())?,
                "Theme was removed while opening it: {id}"
            );
        }
        if named_path.is_none() {
            match id {
                "tui" => {}
                "legacy" => {
                    ensure!(
                        path_present(&self.legacy_path)?,
                        "Legacy theme does not exist: {}",
                        self.legacy_path.display()
                    );
                    let source = ConfigDocument::load(self.legacy_path.clone())?;
                    document.replace_text(&source.text())?;
                    legacy_source = Some(source);
                }
                _ => anyhow::bail!("Theme does not exist: {id}"),
            }
        }
        Styles::deserialize(&document.text()).with_context(|| format!("Invalid theme {id:?}"))?;
        let label = match id {
            "tui" | "legacy" => label_for_id(id),
            _ => display_name(&document, id)?,
        };
        let baseline_text = document.text();
        Ok(ThemeDocument {
            id: id.to_owned(),
            label,
            document,
            legacy_source,
            baseline_text,
            new_theme: false,
            store: self.clone(),
        })
    }

    pub fn create(&self, name: &str, source: &str) -> Result<ThemeDocument> {
        let id = normalize_name(name)?;
        ensure!(
            id != "tui" && id != "legacy",
            "This theme name is reserved: {name}"
        );
        ensure!(
            !self.files()?.contains_key(&id),
            "Theme already exists: {name}"
        );
        Styles::deserialize(source).context("The source theme is invalid")?;
        let mut document = ConfigDocument::load(self.directory.join(format!("theme_{id}.toml")))?;
        ensure!(
            !path_present(document.path())?,
            "Theme already exists: {name}"
        );
        document.replace_text(source)?;
        let label = name.split_whitespace().collect::<Vec<_>>().join(" ");
        document.set(&["theme", "name"], Some(toml::Value::String(label.clone())))?;
        Ok(ThemeDocument {
            id,
            label,
            baseline_text: document.text(),
            document,
            legacy_source: None,
            new_theme: true,
            store: self.clone(),
        })
    }

    pub fn write_defaults(&self) -> Result<PathBuf> {
        ensure!(
            !self.files()?.contains_key("tui"),
            "Tui theme file already exists: {}",
            self.directory.join("theme_tui.toml").display()
        );
        let document = ConfigDocument::load(self.directory.join("theme_tui.toml"))?;
        ensure!(
            !path_present(document.path())?,
            "Tui theme file already exists"
        );
        let mut theme = ThemeDocument {
            id: "tui".into(),
            label: label_for_id("tui"),
            baseline_text: document.text(),
            document,
            legacy_source: None,
            new_theme: true,
            store: self.clone(),
        };
        theme.document.replace_text(&Styles::serialize_default()?)?;
        theme.save()?;
        Ok(theme.path().to_path_buf())
    }

    fn files(&self) -> Result<BTreeMap<String, Vec<PathBuf>>> {
        let entries = match fs::read_dir(&self.directory) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(BTreeMap::new());
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "Reading themes directory failed: {}",
                        self.directory.display()
                    )
                });
            }
        };
        let mut files: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
        for entry in entries {
            let entry = entry.context("Reading theme directory entry failed")?;
            let name = entry.file_name();
            let Some(name) = name.to_str() else { continue };
            let Some(id) = name
                .strip_prefix("theme_")
                .and_then(|name| name.strip_suffix(".toml"))
            else {
                continue;
            };
            files
                .entry(id.to_ascii_lowercase())
                .or_default()
                .push(entry.path());
        }
        Ok(files)
    }
}

impl ThemeDocument {
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn label(&self) -> &str {
        &self.label
    }
    pub fn path(&self) -> &Path {
        self.document.path()
    }
    pub fn text(&self) -> String {
        self.document.text()
    }
    pub fn value(&self, path: &[&str]) -> Option<toml::Value> {
        self.document.value(path)
    }
    pub fn set(&mut self, path: &[&str], value: Option<toml::Value>) -> Result<()> {
        self.document.set(path, value)
    }
    pub fn is_dirty(&self) -> bool {
        self.new_theme || self.text() != self.baseline_text
    }

    pub fn save(&mut self) -> Result<()> {
        self.document.verify_unchanged()?;
        Styles::deserialize(&self.text()).context("The edited theme is invalid")?;
        if let Some(source) = &self.legacy_source {
            source
                .verify_unchanged()
                .context("The legacy theme changed; reopen it before saving")?;
        }
        let paths = self.store.files()?.remove(&self.id).unwrap_or_default();
        ensure!(
            paths.iter().all(|path| path == self.path()) && paths.len() <= 1,
            "Another theme file now uses the same name; reopen it before saving: {}",
            self.id
        );
        if !self.is_dirty() {
            return Ok(());
        }
        self.document.save()?;
        self.baseline_text = self.text();
        self.new_theme = false;
        self.legacy_source = None;
        Ok(())
    }
}

fn normalize_name(name: &str) -> Result<String> {
    ensure!(
        name.chars()
            .all(|character| character.is_ascii_alphanumeric()
                || matches!(character, ' ' | '_' | '-')),
        "Theme names may contain ASCII letters, numbers, spaces, underscores, and hyphens"
    );
    let id = name
        .split_whitespace()
        .collect::<Vec<_>>()
        .join("_")
        .to_ascii_lowercase();
    validate_id(&id)?;
    Ok(id)
}

fn path_present(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => {
            Err(error).with_context(|| format!("Inspecting theme path failed: {}", path.display()))
        }
    }
}

fn validate_id(id: &str) -> Result<()> {
    ensure!(
        !id.is_empty()
            && id.len() <= 64
            && id.bytes().any(|byte| byte.is_ascii_alphanumeric())
            && id.bytes().all(|byte| byte.is_ascii_lowercase()
                || byte.is_ascii_digit()
                || matches!(byte, b'_' | b'-')),
        "Invalid theme name: use 1–64 lowercase ASCII letters, numbers, underscores, or hyphens"
    );
    Ok(())
}

fn label_for_id(id: &str) -> String {
    match id {
        "tui" => "Tui (default)".into(),
        "legacy" => "Legacy".into(),
        _ => id.replace('_', " "),
    }
}

fn display_name(document: &ConfigDocument, id: &str) -> Result<String> {
    match document.value(&["theme", "name"]) {
        None => Ok(label_for_id(id)),
        Some(toml::Value::String(name)) => {
            ensure!(
                normalize_name(&name)? == id,
                "Theme display name does not match its filename: {id}"
            );
            Ok(name)
        }
        Some(_) => anyhow::bail!("Theme display name must be a string: {id}"),
    }
}

#[cfg(test)]
mod tests {
    use ratatui::style::Color;

    use super::*;

    fn store() -> (tempfile::TempDir, ThemeStore) {
        let directory = tempfile::tempdir().unwrap();
        let store = ThemeStore::new(Some(&directory.path().to_path_buf())).unwrap();
        (directory, store)
    }

    fn set_color(document: &mut ThemeDocument, color: &str) {
        document
            .set(&["general", "input_block_active", "fg"], Some(color.into()))
            .unwrap();
    }

    #[test]
    fn browsing_opening_and_cancelling_themes_create_nothing() {
        let (directory, store) = store();
        let themes = store.list().unwrap();
        assert_eq!(themes.len(), 1);
        assert_eq!(themes[0].id, "tui");
        assert_eq!(themes[0].label, "Tui (default)");
        assert!(!store.load("tui").unwrap().is_dirty());
        assert_eq!(store.resolve(None).id.as_deref(), Some("tui"));
        let mut new_theme = store.create("Night Sky", "").unwrap();
        assert_eq!(new_theme.id(), "night_sky");
        assert!(new_theme.is_dirty());
        set_color(&mut new_theme, "Blue");
        drop(new_theme);
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[test]
    fn custom_directory_and_legacy_file_config_keep_their_theme_paths() {
        let (directory, store) = store();
        assert_eq!(store.directory(), directory.path().join("themes"));
        let config_file = directory.path().join("config.toml");
        fs::write(&config_file, "").unwrap();
        let legacy_config = ThemeStore::new(Some(&config_file)).unwrap();
        assert_eq!(
            legacy_config.directory(),
            settings_default_dir_path().unwrap().join("themes")
        );
    }

    #[test]
    fn new_theme_saves_named_metadata_and_reloads() {
        let (_directory, store) = store();
        let mut theme = store.create("  Night   Sky  ", "").unwrap();
        assert_eq!(theme.label(), "Night Sky");
        assert!(theme.path().ends_with("themes/theme_night_sky.toml"));
        theme.save().unwrap();
        assert!(!theme.is_dirty());
        let loaded = store.load("night_sky").unwrap();
        assert_eq!(loaded.label(), "Night Sky");
        assert!(!loaded.is_dirty());
        assert_eq!(store.list().unwrap().len(), 2);
    }

    #[test]
    fn tui_edits_write_only_changed_values_without_changing_compiled_defaults() {
        let (_directory, store) = store();
        let original = Styles::serialize_default().unwrap();
        let mut theme = store.load("tui").unwrap();
        set_color(&mut theme, "Red");
        theme.save().unwrap();
        assert_eq!(
            store.resolve(None).styles.general.input_block_active.fg,
            Some(Color::Red)
        );
        assert_eq!(original, Styles::serialize_default().unwrap());
        assert!(fs::read_to_string(theme.path()).unwrap().contains("Red"));
    }

    #[test]
    fn write_defaults_creates_complete_tui_and_refuses_overwrite() {
        let (_directory, store) = store();
        let path = store.write_defaults().unwrap();
        let defaults = Styles::serialize_default().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), defaults);
        fs::write(&path, "# keep this file\n").unwrap();
        assert!(store.write_defaults().is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "# keep this file\n");
    }

    #[test]
    fn writing_tui_defaults_preserves_legacy_selection_and_source() {
        let (directory, store) = store();
        let source = "[msgbox]\nerror = 'Blue'\n";
        fs::write(directory.path().join("themes.toml"), source).unwrap();
        store.write_defaults().unwrap();
        assert_eq!(store.default_id(), "legacy");
        assert_eq!(store.resolve(None).styles.msgbox.error, Color::Blue);
        assert_eq!(
            fs::read_to_string(directory.path().join("themes.toml")).unwrap(),
            source
        );
    }

    #[test]
    fn legacy_open_is_clean_and_first_edit_migrates_without_changing_source() {
        let (directory, store) = store();
        let source = "# Keep my comment\nunknown = 'untouched'\n[general.input_corsur_active]\nfg = 'Blue'\n";
        let original_path = directory.path().join("themes.toml");
        fs::write(&original_path, source).unwrap();
        assert_eq!(store.resolve(None).id.as_deref(), Some("legacy"));
        let mut theme = store.load("legacy").unwrap();
        assert_eq!(theme.label(), "Legacy");
        assert!(!theme.is_dirty());
        theme.save().unwrap();
        assert!(!store.directory().exists());
        set_color(&mut theme, "Red");
        theme.save().unwrap();
        assert_eq!(fs::read_to_string(&original_path).unwrap(), source);
        let saved = fs::read_to_string(theme.path()).unwrap();
        assert!(saved.contains("# Keep my comment"));
        assert!(saved.contains("unknown = 'untouched'"));
        assert_eq!(
            Styles::deserialize(&saved)
                .unwrap()
                .general
                .input_cursor_active
                .fg,
            Some(Color::Blue)
        );
        fs::remove_file(&original_path).unwrap();
        assert_eq!(store.resolve(None).id.as_deref(), Some("legacy"));
        assert_eq!(
            store.resolve(None).styles.general.input_block_active.fg,
            Some(Color::Red)
        );
    }

    #[test]
    fn migrated_legacy_file_takes_precedence_over_original() {
        let (directory, store) = store();
        fs::create_dir(store.directory()).unwrap();
        fs::write(
            directory.path().join("themes.toml"),
            "[msgbox]\nerror = 'Blue'\n",
        )
        .unwrap();
        fs::write(
            store.directory().join("theme_legacy.toml"),
            "[msgbox]\nerror = 'Red'\n",
        )
        .unwrap();
        assert_eq!(store.resolve(None).styles.msgbox.error, Color::Red);
        assert_eq!(store.list().unwrap().len(), 2);
    }

    #[test]
    fn modified_legacy_source_blocks_migration() {
        let (directory, store) = store();
        let path = directory.path().join("themes.toml");
        fs::write(&path, "# original\n").unwrap();
        let mut theme = store.load("legacy").unwrap();
        set_color(&mut theme, "Red");
        fs::write(&path, "# external edit\n").unwrap();
        assert!(theme.save().is_err());
        assert!(!theme.path().exists());
        assert_eq!(fs::read_to_string(path).unwrap(), "# external edit\n");
    }

    #[test]
    fn modified_named_file_blocks_save() {
        let (_directory, store) = store();
        let mut theme = store.create("Night", "").unwrap();
        theme.save().unwrap();
        set_color(&mut theme, "Red");
        fs::write(theme.path(), "# external edit\n").unwrap();
        assert!(theme.save().is_err());
        assert_eq!(
            fs::read_to_string(theme.path()).unwrap(),
            "# external edit\n"
        );
        assert!(theme.is_dirty());
    }

    #[test]
    fn unchanged_named_draft_detects_external_changes_before_applying() {
        let (_directory, store) = store();
        let mut theme = store.create("Night", "").unwrap();
        theme.save().unwrap();
        fs::write(theme.path(), "# external edit\n").unwrap();
        assert!(!theme.is_dirty());
        assert!(theme.save().is_err());
    }

    #[test]
    fn unchanged_legacy_draft_detects_external_changes_without_migrating() {
        let (directory, store) = store();
        let path = directory.path().join("themes.toml");
        fs::write(&path, "# original\n").unwrap();
        let mut theme = store.load("legacy").unwrap();
        fs::write(&path, "# external edit\n").unwrap();
        assert!(!theme.is_dirty());
        assert!(theme.save().is_err());
        assert!(!store.directory().exists());
    }

    #[test]
    fn created_after_draft_file_is_never_overwritten() {
        let (_directory, store) = store();
        let mut theme = store.create("Night", "").unwrap();
        fs::create_dir(store.directory()).unwrap();
        fs::write(theme.path(), "# somebody else created it\n").unwrap();
        assert!(theme.save().is_err());
        assert_eq!(
            fs::read_to_string(theme.path()).unwrap(),
            "# somebody else created it\n"
        );
    }

    #[test]
    fn unsafe_and_reserved_names_are_rejected_without_files() {
        let (_directory, store) = store();
        for name in [
            "",
            " ",
            "..",
            "../escape",
            "a/b",
            "a\\b",
            "night.toml",
            "a\nb",
            "a\tb",
            "é",
            "---",
            "TUI",
            "Legacy",
        ] {
            assert!(store.create(name, "").is_err(), "accepted {name:?}");
        }
        assert!(store.create(&"n".repeat(65), "").is_err());
        assert!(store.load("../escape").is_err());
        assert!(!store.directory().exists());
    }

    #[test]
    fn existing_names_are_rejected_case_insensitively() {
        let (_directory, store) = store();
        fs::create_dir(store.directory()).unwrap();
        fs::write(store.directory().join("theme_NIGHT.toml"), "").unwrap();
        assert!(store.create("Night", "").is_err());
        assert_eq!(store.load("night").unwrap().id(), "night");
        fs::write(store.directory().join("theme_TUI.toml"), "").unwrap();
        assert!(store.write_defaults().is_err());
    }

    #[test]
    fn case_collision_after_draft_blocks_save() {
        let (_directory, store) = store();
        let mut theme = store.create("Night", "").unwrap();
        fs::create_dir(store.directory()).unwrap();
        let external = store.directory().join("theme_NIGHT.toml");
        fs::write(&external, "# preserve\n").unwrap();
        assert!(theme.save().is_err());
        assert_eq!(fs::read_to_string(external).unwrap(), "# preserve\n");
    }

    #[test]
    fn list_reports_invalid_theme_files_without_hiding_valid_ones() {
        let (_directory, store) = store();
        fs::create_dir(store.directory()).unwrap();
        fs::write(
            store.directory().join("theme_broken.toml"),
            "[msgbox]\nerror = 'invalid'\n",
        )
        .unwrap();
        fs::write(store.directory().join("theme_bad.name.toml"), "").unwrap();
        fs::write(store.directory().join("unrelated.toml"), "").unwrap();
        let themes = store.list().unwrap();
        assert_eq!(themes.len(), 3);
        assert_eq!(themes[0].id, "tui");
        assert!(themes[0].error.is_none());
        assert!(themes[1..].iter().all(|theme| theme.error.is_some()));
    }

    #[test]
    fn invalid_selection_falls_back_to_compiled_tui_without_writing() {
        let (directory, store) = store();
        let settings_path = directory.path().join("config.toml");
        let settings = "theme = 'missing'\n";
        fs::write(&settings_path, settings).unwrap();
        fs::create_dir(store.directory()).unwrap();
        fs::write(
            store.directory().join("theme_tui.toml"),
            "[msgbox]\nerror = 'Blue'\n",
        )
        .unwrap();
        let missing = store.resolve(Some("missing"));
        assert_eq!(missing.id, None);
        assert!(missing.warning.is_some());
        assert_eq!(missing.styles.msgbox.error, Styles::default().msgbox.error);
        assert_eq!(fs::read_to_string(&settings_path).unwrap(), settings);
        fs::write(store.directory().join("theme_tui.toml"), "invalid = [").unwrap();
        let invalid = store.resolve(Some("tui"));
        assert!(invalid.warning.is_some());
        assert_eq!(invalid.id, None);
        assert_eq!(
            fs::read_to_string(store.directory().join("theme_tui.toml")).unwrap(),
            "invalid = ["
        );
    }

    #[test]
    fn malformed_or_mismatched_display_names_are_invalid() {
        let (_directory, store) = store();
        fs::create_dir(store.directory()).unwrap();
        for contents in [
            "[theme]\nname = 1",
            "[theme]\nname = 'Other'",
            "[theme]\nname = 'Night\\nOops'",
        ] {
            fs::write(store.directory().join("theme_night.toml"), contents).unwrap();
            assert!(store.load("night").is_err());
        }
    }

    #[test]
    fn invalid_edited_styles_cannot_replace_a_valid_theme() {
        let (_directory, store) = store();
        let mut theme = store.create("Night", "").unwrap();
        theme.save().unwrap();
        let original = fs::read_to_string(theme.path()).unwrap();
        set_color(&mut theme, "invalid color");
        assert!(theme.save().is_err());
        assert_eq!(fs::read_to_string(theme.path()).unwrap(), original);
    }

    #[cfg(unix)]
    #[test]
    fn legacy_symlink_retarget_blocks_migration() {
        use std::os::unix::fs::symlink;

        let (directory, store) = store();
        let first = directory.path().join("first.toml");
        let second = directory.path().join("second.toml");
        fs::write(&first, "# same text\n").unwrap();
        fs::write(&second, "# same text\n").unwrap();
        let legacy = directory.path().join("themes.toml");
        symlink(&first, &legacy).unwrap();
        let mut theme = store.load("legacy").unwrap();
        set_color(&mut theme, "Red");
        fs::remove_file(&legacy).unwrap();
        symlink(&second, &legacy).unwrap();
        assert!(theme.save().is_err());
        assert!(!theme.path().exists());
    }
}

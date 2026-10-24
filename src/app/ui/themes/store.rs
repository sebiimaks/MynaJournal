use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};

use crate::settings::{document::ConfigDocument, settings_default_dir_path};

use super::{Styles, bundled};

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

    /// Install missing built-in themes without replacing any existing files.
    pub fn initialize(&self) -> Result<()> {
        let tui = Styles::serialize_default()?;
        for (id, source) in
            std::iter::once(("tui", tui.as_str())).chain(bundled::THEMES.iter().copied())
        {
            let path = self.directory.join(format!("theme_{id}.toml"));
            if self.files()?.contains_key(id) || path_present(&path)? {
                continue;
            }
            let mut document = ConfigDocument::load(path.clone())?;
            if path_present(&path)? {
                continue;
            }
            document.replace_text(source)?;
            if let Err(error) = document.save() {
                // Another process may have installed the file while this one prepared it.
                if self.files()?.contains_key(id) || path_present(&path)? {
                    continue;
                }
                return Err(error).with_context(|| format!("Installing theme {id:?} failed"));
            }
        }
        Ok(())
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
        for &(id, _) in bundled::THEMES {
            files.entry(id.into()).or_default();
        }
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
            ensure!(
                !path_present(document.path())?,
                "Theme appeared while opening it; reopen it before editing: {id}"
            );
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
                _ => match bundled::source(id) {
                    Some(source) => document.replace_text(source)?,
                    None => anyhow::bail!("Theme does not exist: {id}"),
                },
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
        let automatic_name;
        let name = if name.bytes().all(|byte| byte == b' ') {
            automatic_name = self.next_custom_name()?;
            &automatic_name
        } else {
            name
        };
        let id = normalize_name(name)?;
        ensure!(
            id != "tui" && id != "legacy" && bundled::source(&id).is_none(),
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

    pub fn write_new(&self, name: &str) -> Result<PathBuf> {
        let mut theme = self.create(name, &Styles::serialize_default()?)?;
        theme.save()?;
        Ok(theme.path().to_path_buf())
    }

    fn next_custom_name(&self) -> Result<String> {
        let files = self.files()?;
        for index in 0u64.. {
            let (id, name) = if index == 0 {
                ("tui_custom".into(), "Tui Custom".into())
            } else {
                (
                    format!("tui_custom_{index:02}"),
                    format!("Tui Custom {index:02}"),
                )
            };
            if !files.contains_key(&id)
                && !path_present(&self.directory.join(format!("theme_{id}.toml")))?
            {
                return Ok(name);
            }
        }
        anyhow::bail!("No unused custom theme name is available")
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
        assert_eq!(themes.len(), 1 + bundled::THEMES.len());
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
    fn all_supplied_themes_are_available_without_files_or_a_changed_default() {
        let (directory, store) = store();
        let catalog = store.list().unwrap();
        let ids: Vec<_> = catalog.iter().map(|theme| theme.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "tui",
                "dark_abyssal",
                "dark_borealis",
                "dark_fernwood",
                "dark_kiln",
                "dark_night_market",
                "light_cyanotype",
                "light_lido",
                "light_matcha",
                "light_riso",
                "light_sorbet",
            ]
        );
        assert!(catalog.iter().all(|theme| theme.error.is_none()));
        for theme in &catalog[1..] {
            let mut document = store.load(&theme.id).unwrap();
            assert!(!document.is_dirty());
            document.save().unwrap();
            let resolved = store.resolve(Some(&theme.id));
            assert_eq!(resolved.id.as_deref(), Some(theme.id.as_str()));
            assert!(resolved.warning.is_none());
            assert_eq!(resolved.styles.tags.palette.len(), 8);
        }
        assert_eq!(store.resolve(None).id.as_deref(), Some("tui"));
        assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[test]
    fn editing_bundled_theme_saves_an_override_and_preserves_its_other_values() {
        let (_directory, store) = store();
        let original = store.load("dark_abyssal").unwrap().text();
        let original_styles = Styles::deserialize(&original).unwrap();
        let mut theme = store.load("dark_abyssal").unwrap();
        set_color(&mut theme, "Red");
        theme.save().unwrap();
        assert!(!theme.is_dirty());
        assert!(theme.path().ends_with("themes/theme_dark_abyssal.toml"));
        assert_eq!(fs::read_dir(store.directory()).unwrap().count(), 1);
        let saved = fs::read_to_string(theme.path()).unwrap();
        assert!(saved.contains("# Abyssal for tui-journal"));
        let reloaded = store.resolve(Some("dark_abyssal"));
        assert_eq!(
            reloaded.styles.general.input_block_active.fg,
            Some(Color::Red)
        );
        assert_eq!(reloaded.styles.tags, original_styles.tags);
        assert_eq!(reloaded.styles.editor, original_styles.editor);
        assert_eq!(
            store
                .list()
                .unwrap()
                .iter()
                .filter(|theme| theme.id == "dark_abyssal")
                .count(),
            1
        );
        fs::remove_file(theme.path()).unwrap();
        assert_eq!(store.load("dark_abyssal").unwrap().text(), original);
    }

    #[test]
    fn existing_bundled_name_file_replaces_builtin_values_without_being_overwritten() {
        let (_directory, store) = store();
        fs::create_dir(store.directory()).unwrap();
        let path = store.directory().join("theme_dark_abyssal.toml");
        let existing = "# My existing custom theme\n[general.input_block_active]\nfg = 'Red'\n";
        fs::write(&path, existing).unwrap();
        let resolved = store.resolve(Some("dark_abyssal"));
        assert_eq!(resolved.id.as_deref(), Some("dark_abyssal"));
        assert_eq!(
            resolved.styles.general.input_block_active.fg,
            Some(Color::Red)
        );
        assert_eq!(resolved.styles.tags, Styles::default().tags);
        store.load("dark_abyssal").unwrap().save().unwrap();
        assert_eq!(fs::read_to_string(path).unwrap(), existing);
    }

    #[test]
    fn invalid_bundled_override_is_reported_instead_of_silently_replaced() {
        let (_directory, store) = store();
        fs::create_dir(store.directory()).unwrap();
        let path = store.directory().join("theme_dark_abyssal.toml");
        fs::write(&path, "invalid = [").unwrap();
        let catalog = store.list().unwrap();
        assert!(
            catalog
                .iter()
                .find(|theme| theme.id == "dark_abyssal")
                .unwrap()
                .error
                .is_some()
        );
        let resolved = store.resolve(Some("dark_abyssal"));
        assert!(resolved.warning.is_some());
        assert_eq!(resolved.id, None);
        assert_eq!(resolved.styles.tags, Styles::default().tags);
        assert_eq!(fs::read_to_string(path).unwrap(), "invalid = [");
    }

    #[test]
    fn bundled_theme_names_are_reserved_but_copying_to_a_new_name_is_allowed() {
        let (_directory, store) = store();
        for &(id, _) in bundled::THEMES {
            assert!(
                store
                    .create(&id.replace('_', " ").to_uppercase(), "")
                    .is_err()
            );
        }
        let source = store.load("light_sorbet").unwrap();
        let mut copy = store.create("My Sorbet", &source.text()).unwrap();
        copy.save().unwrap();
        let loaded = store.load("my_sorbet").unwrap();
        assert_eq!(loaded.label(), "My Sorbet");
        assert_eq!(
            Styles::deserialize(&loaded.text()).unwrap().tags,
            Styles::deserialize(&source.text()).unwrap().tags
        );
        assert!(!source.path().exists());
    }

    #[test]
    fn file_created_after_opening_a_bundled_theme_blocks_its_first_save() {
        let (_directory, store) = store();
        let mut theme = store.load("light_matcha").unwrap();
        set_color(&mut theme, "Red");
        fs::create_dir(store.directory()).unwrap();
        fs::write(theme.path(), "# Created externally\n").unwrap();
        assert!(theme.save().is_err());
        assert!(theme.is_dirty());
        assert_eq!(
            fs::read_to_string(theme.path()).unwrap(),
            "# Created externally\n"
        );
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
        assert_eq!(store.list().unwrap().len(), 2 + bundled::THEMES.len());
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
    fn initialization_installs_all_defaults_and_preserves_existing_edits() {
        let (_directory, store) = store();
        store.initialize().unwrap();
        let tui_path = store.directory().join("theme_tui.toml");
        assert_eq!(
            fs::read_to_string(&tui_path).unwrap(),
            Styles::serialize_default().unwrap()
        );
        for &(id, source) in bundled::THEMES {
            assert_eq!(
                fs::read_to_string(store.directory().join(format!("theme_{id}.toml"))).unwrap(),
                source
            );
        }
        assert_eq!(fs::read_dir(store.directory()).unwrap().count(), 11);
        fs::write(&tui_path, "# Customized Tui\n[msgbox]\nerror = 'Blue'\n").unwrap();
        let modified = fs::metadata(&tui_path).unwrap().modified().unwrap();
        store.initialize().unwrap();
        assert_eq!(
            fs::read_to_string(&tui_path).unwrap(),
            "# Customized Tui\n[msgbox]\nerror = 'Blue'\n"
        );
        assert_eq!(
            fs::metadata(&tui_path).unwrap().modified().unwrap(),
            modified
        );
        let removed = store.directory().join("theme_light_sorbet.toml");
        fs::remove_file(&removed).unwrap();
        store.initialize().unwrap();
        assert_eq!(
            fs::read_to_string(removed).unwrap(),
            bundled::source("light_sorbet").unwrap()
        );
        assert_eq!(
            fs::metadata(&tui_path).unwrap().modified().unwrap(),
            modified
        );
    }

    #[test]
    fn initializing_themes_preserves_legacy_selection_source_and_settings() {
        let (directory, store) = store();
        let source = "[msgbox]\nerror = 'Blue'\n";
        fs::write(directory.path().join("themes.toml"), source).unwrap();
        let config = "# Keep my preferences\nauto_title = { kind = 'date' }\n";
        fs::write(directory.path().join("config.toml"), config).unwrap();
        store.initialize().unwrap();
        assert_eq!(store.default_id(), "legacy");
        assert_eq!(store.resolve(None).styles.msgbox.error, Color::Blue);
        assert_eq!(
            fs::read_to_string(directory.path().join("themes.toml")).unwrap(),
            source
        );
        assert_eq!(
            fs::read_to_string(directory.path().join("config.toml")).unwrap(),
            config
        );
        assert!(!store.directory().join("theme_legacy.toml").exists());
    }

    #[test]
    fn initialization_keeps_case_variants_and_invalid_existing_files() {
        let (_directory, store) = store();
        fs::create_dir(store.directory()).unwrap();
        let tui = store.directory().join("theme_TUI.toml");
        let dark = store.directory().join("theme_dark_abyssal.toml");
        fs::write(&tui, "# Preserve case and contents\n").unwrap();
        fs::write(&dark, "invalid = [").unwrap();
        store.initialize().unwrap();
        assert_eq!(
            fs::read_to_string(tui).unwrap(),
            "# Preserve case and contents\n"
        );
        assert_eq!(fs::read_to_string(dark).unwrap(), "invalid = [");
        assert_eq!(fs::read_dir(store.directory()).unwrap().count(), 11);
        assert!(
            store
                .list()
                .unwrap()
                .iter()
                .find(|theme| theme.id == "dark_abyssal")
                .unwrap()
                .error
                .is_some()
        );
    }

    #[cfg(unix)]
    #[test]
    fn initialization_does_not_replace_existing_or_dangling_theme_symlinks() {
        use std::os::unix::fs::symlink;
        let (directory, store) = store();
        fs::create_dir(store.directory()).unwrap();
        let existing = directory.path().join("existing.toml");
        let missing = directory.path().join("missing.toml");
        fs::write(&existing, "# Linked customization\n").unwrap();
        let tui = store.directory().join("theme_tui.toml");
        let dark = store.directory().join("theme_dark_abyssal.toml");
        symlink(&existing, &tui).unwrap();
        symlink(&missing, &dark).unwrap();
        store.initialize().unwrap();
        assert_eq!(fs::read_link(tui).unwrap(), existing);
        assert_eq!(fs::read_link(dark).unwrap(), missing);
        assert_eq!(
            fs::read_to_string(existing).unwrap(),
            "# Linked customization\n"
        );
        assert!(!missing.exists());
    }

    #[test]
    fn unusable_themes_directory_reports_initialization_failure_without_changing_it() {
        let (_directory, store) = store();
        fs::write(store.directory(), "Keep this file").unwrap();
        assert!(store.initialize().is_err());
        assert_eq!(
            fs::read_to_string(store.directory()).unwrap(),
            "Keep this file"
        );
    }

    #[test]
    fn unnamed_themes_use_base_then_underscored_numbers_and_preserve_source_styles() {
        let (_directory, store) = store();
        let source = store.load("dark_abyssal").unwrap().text();
        for (input, id, label) in [
            ("", "tui_custom", "Tui Custom"),
            (" ", "tui_custom_01", "Tui Custom 01"),
            ("", "tui_custom_02", "Tui Custom 02"),
        ] {
            let mut theme = store.create(input, &source).unwrap();
            assert_eq!(theme.id(), id);
            assert_eq!(theme.label(), label);
            assert!(!theme.path().exists());
            theme.save().unwrap();
            let reloaded = store.load(id).unwrap();
            assert_eq!(reloaded.label(), label);
            assert_eq!(
                Styles::deserialize(&reloaded.text()).unwrap().tags,
                Styles::deserialize(&source).unwrap().tags
            );
        }
    }

    #[test]
    fn automatic_names_skip_occupied_case_variants_and_use_the_first_unused_suffix() {
        let (_directory, store) = store();
        fs::create_dir(store.directory()).unwrap();
        fs::write(store.directory().join("theme_TUI_CUSTOM.toml"), "# Keep\n").unwrap();
        fs::create_dir(store.directory().join("theme_tui_custom_01.toml")).unwrap();
        fs::write(
            store.directory().join("theme_tui_custom_03.toml"),
            "invalid = [",
        )
        .unwrap();
        let path = store.write_new("").unwrap();
        assert!(path.ends_with("theme_tui_custom_02.toml"));
        assert_eq!(
            fs::read_to_string(store.directory().join("theme_TUI_CUSTOM.toml")).unwrap(),
            "# Keep\n"
        );
        assert_eq!(
            fs::read_to_string(store.directory().join("theme_tui_custom_03.toml")).unwrap(),
            "invalid = ["
        );
    }

    #[test]
    fn write_new_always_uses_pristine_tui_values_and_never_overwrites_named_files() {
        let (_directory, store) = store();
        store.initialize().unwrap();
        let mut tui = store.load("tui").unwrap();
        set_color(&mut tui, "Red");
        tui.save().unwrap();
        let path = store.write_new("  Night   Blue  ").unwrap();
        assert!(path.ends_with("theme_night_blue.toml"));
        let loaded = store.load("night_blue").unwrap();
        assert_eq!(loaded.label(), "Night Blue");
        assert_eq!(
            toml::Value::try_from(Styles::deserialize(&loaded.text()).unwrap()).unwrap(),
            toml::Value::try_from(Styles::default()).unwrap()
        );
        let original = fs::read_to_string(&path).unwrap();
        assert!(store.write_new("NIGHT BLUE").is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), original);
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
        assert_eq!(store.list().unwrap().len(), 2 + bundled::THEMES.len());
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
        assert!(store.write_new("Tui").is_err());
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
        assert_eq!(themes.len(), 3 + bundled::THEMES.len());
        assert_eq!(themes[0].id, "tui");
        assert!(themes[0].error.is_none());
        assert_eq!(
            themes.iter().filter(|theme| theme.error.is_some()).count(),
            2
        );
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

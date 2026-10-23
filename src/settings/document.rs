use std::{
    fs::{self, File, Metadata, Permissions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::SystemTime,
};

use anyhow::{Context, Result, bail, ensure};
use toml_edit::{DocumentMut, InlineTable, Item, Table, TableLike, Value};

#[derive(Clone, Debug)]
pub struct ConfigDocument {
    path: PathBuf,
    document: DocumentMut,
    baseline: Snapshot,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Snapshot {
    target: PathBuf,
    source: Option<String>,
    file: Option<FileIdentity>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct FileIdentity {
    length: u64,
    modified: Option<SystemTime>,
    permissions: Permissions,
    #[cfg(unix)]
    device: u64,
    #[cfg(unix)]
    inode: u64,
}

impl FileIdentity {
    fn from_metadata(metadata: &Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;

        Self {
            length: metadata.len(),
            modified: metadata.modified().ok(),
            permissions: metadata.permissions(),
            #[cfg(unix)]
            device: metadata.dev(),
            #[cfg(unix)]
            inode: metadata.ino(),
        }
    }
}

impl ConfigDocument {
    pub fn load(path: PathBuf) -> Result<Self> {
        let path = std::path::absolute(path).context("Resolving configuration path failed")?;
        let baseline = Snapshot::read(&path)?;
        let document = baseline
            .source
            .as_deref()
            .unwrap_or_default()
            .parse::<DocumentMut>()
            .with_context(|| format!("Parsing configuration failed: {}", path.display()))?;
        Ok(Self {
            path,
            document,
            baseline,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn text(&self) -> String {
        self.document.to_string()
    }

    pub fn replace_text(&mut self, text: &str) -> Result<()> {
        let document = text.parse::<DocumentMut>()?;
        self.document = document;
        Ok(())
    }

    /// Persist one preference without committing unrelated edits in this document.
    pub fn save_field_preserving_draft(
        &mut self,
        path: &[&str],
        value: Option<toml::Value>,
    ) -> Result<()> {
        self.verify_unchanged()?;
        let mut saved = self.clone();
        saved.document = self
            .baseline
            .source
            .as_deref()
            .unwrap_or_default()
            .parse()?;
        saved.set(path, value.clone())?;
        let mut draft = self.clone();
        draft.set(path, value)?;
        saved.save()?;
        draft.baseline = saved.baseline;
        *self = draft;
        Ok(())
    }

    pub fn value(&self, path: &[&str]) -> Option<toml::Value> {
        let mut document = self.document.clone();
        normalize_alias(&mut document, path);
        value_at(&document, path)
    }

    pub fn set(&mut self, path: &[&str], value: Option<toml::Value>) -> Result<()> {
        ensure!(!path.is_empty(), "A configuration field must be specified");
        let mut document = self.document.clone();
        normalize_alias(&mut document, path);
        if value_at(&document, path).as_ref() == value.as_ref() {
            self.document = document;
            return Ok(());
        }
        let value = value.map(to_edit_value).transpose()?.map(Item::Value);
        set_item(document.as_table_mut(), path, value, path)?;
        // Validate the representation after transitions between scalar and table forms.
        document.to_string().parse::<DocumentMut>()?;
        self.document = document;
        Ok(())
    }

    pub fn is_dirty(&self) -> bool {
        self.text() != self.baseline.source.as_deref().unwrap_or_default()
    }

    pub fn save(&mut self) -> Result<()> {
        self.verify_unchanged()?;
        if !self.is_dirty() {
            return Ok(());
        }
        let parent = self
            .baseline
            .target
            .parent()
            .context("Configuration path has no parent directory")?;
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "Creating configuration directory failed: {}",
                parent.display()
            )
        })?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent)
            .context("Creating temporary configuration file failed")?;
        let text = self.text();
        temporary
            .write_all(text.as_bytes())
            .context("Writing configuration failed")?;
        if let Some(file) = &self.baseline.file {
            temporary
                .as_file()
                .set_permissions(file.permissions.clone())
                .context("Preserving configuration permissions failed")?;
        }
        temporary
            .as_file()
            .sync_all()
            .context("Flushing configuration failed")?;
        let identity = FileIdentity::from_metadata(&temporary.as_file().metadata()?);
        self.verify_unchanged()?;
        if self.baseline.source.is_some() {
            temporary.persist(&self.baseline.target)
        } else {
            temporary.persist_noclobber(&self.baseline.target)
        }
        .with_context(|| format!("Saving configuration failed: {}", self.path.display()))?;
        self.baseline.source = Some(text);
        self.baseline.file = Some(identity);
        Ok(())
    }

    pub fn verify_unchanged(&self) -> Result<()> {
        let current = Snapshot::read(&self.path).with_context(|| {
            format!(
                "Configuration changed or became inaccessible; reopen it before saving: {}",
                self.path.display()
            )
        })?;
        ensure!(
            current == self.baseline,
            "Configuration changed outside the app; reopen it before saving: {}",
            self.path.display()
        );
        Ok(())
    }
}

impl Snapshot {
    fn read(path: &Path) -> Result<Self> {
        let target = resolve_target(path)?;
        match fs::metadata(&target) {
            Ok(metadata) => {
                ensure!(
                    metadata.is_file(),
                    "Configuration is not a regular file: {}",
                    path.display()
                );
                let mut file = File::open(&target)
                    .with_context(|| format!("Opening configuration failed: {}", path.display()))?;
                let identity = FileIdentity::from_metadata(&file.metadata()?);
                let mut source = String::new();
                file.read_to_string(&mut source)
                    .with_context(|| format!("Reading configuration failed: {}", path.display()))?;
                ensure!(
                    identity == FileIdentity::from_metadata(&file.metadata()?)
                        && identity == FileIdentity::from_metadata(&fs::metadata(&target)?)
                        && resolve_target(path)? == target,
                    "Configuration changed while reading it; reopen it: {}",
                    path.display()
                );
                Ok(Self {
                    target,
                    source: Some(source),
                    file: Some(identity),
                })
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self {
                target,
                source: None,
                file: None,
            }),
            Err(error) => Err(error)
                .with_context(|| format!("Inspecting configuration failed: {}", path.display())),
        }
    }
}

fn resolve_target(path: &Path) -> Result<PathBuf> {
    let mut ancestor = path;
    let mut missing = Vec::new();
    loop {
        match fs::symlink_metadata(ancestor) {
            Ok(_) => {
                let mut target = ancestor.canonicalize().with_context(|| {
                    format!("Resolving configuration target failed: {}", path.display())
                })?;
                for component in missing.iter().rev() {
                    target.push(component);
                }
                return Ok(target);
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing.push(
                    ancestor
                        .file_name()
                        .context("Configuration path has no file name")?,
                );
                ancestor = ancestor
                    .parent()
                    .context("Configuration path has no parent")?;
            }
            Err(error) => {
                return Err(error).with_context(|| {
                    format!("Inspecting configuration path failed: {}", path.display())
                });
            }
        }
    }
}

fn to_edit_value(value: toml::Value) -> Result<Value> {
    Ok(match value {
        toml::Value::String(value) => Value::from(value),
        toml::Value::Integer(value) => Value::from(value),
        toml::Value::Float(value) => Value::from(value),
        toml::Value::Boolean(value) => Value::from(value),
        toml::Value::Datetime(value) => value.to_string().parse()?,
        toml::Value::Array(values) => {
            let mut array = toml_edit::Array::new();
            for value in values {
                array.push(to_edit_value(value)?);
            }
            Value::Array(array)
        }
        toml::Value::Table(values) => {
            let mut table = InlineTable::new();
            for (key, value) in values {
                table.insert(key, to_edit_value(value)?);
            }
            Value::InlineTable(table)
        }
    })
}

fn value_at(document: &DocumentMut, path: &[&str]) -> Option<toml::Value> {
    let document: toml::Value = toml::from_str(&document.to_string()).ok()?;
    if path == ["external_editor", "command"]
        && let Some(command) = document
            .get("external_editor")
            .and_then(toml::Value::as_str)
    {
        return Some(toml::Value::String(command.to_owned()));
    }
    path.iter()
        .try_fold(&document, |value, key| value.get(*key))
        .cloned()
}

fn set_item(
    table: &mut dyn TableLike,
    path: &[&str],
    value: Option<Item>,
    full_path: &[&str],
) -> Result<()> {
    let key = path[0];
    if path.len() == 1 {
        match value {
            Some(mut replacement) => {
                let header_decor = table
                    .get(key)
                    .and_then(Item::as_table)
                    .map(|table| table.decor().clone());
                if let Some(existing) = table.get_mut(key) {
                    if let Some(value) = replacement.as_value_mut() {
                        if let Some(existing) = existing.as_value() {
                            *value.decor_mut() = existing.decor().clone();
                        } else if let Some(suffix) =
                            header_decor.as_ref().and_then(|decor| decor.suffix())
                        {
                            value.decor_mut().set_suffix(suffix.clone());
                        }
                    }
                    *existing = replacement;
                } else {
                    table.insert(key, replacement);
                }
                if let Some(prefix) = header_decor.as_ref().and_then(|decor| decor.prefix())
                    && let Some(mut key) = table.key_mut(key)
                {
                    key.leaf_decor_mut().set_prefix(prefix.clone());
                }
            }
            None => {
                table.remove(key);
            }
        }
        return Ok(());
    }
    if !table.contains_key(key) {
        if value.is_none() {
            return Ok(());
        }
        table.insert(key, Item::Table(Table::new()));
    }
    let item = table.get_mut(key).expect("parent field exists");
    if full_path[0] == "external_editor"
        && path == full_path
        && let Some(command) = item.as_str()
    {
        let mut replacement = InlineTable::new();
        replacement.insert("command", command.into());
        if let Some(value) = item.as_value() {
            *replacement.decor_mut() = value.decor().clone();
        }
        *item = Item::Value(Value::InlineTable(replacement));
    }
    if full_path[0] == "auto_title" && path == full_path && item.is_str() {
        let mut replacement = InlineTable::new();
        if let Some(value) = item.as_value() {
            *replacement.decor_mut() = value.decor().clone();
        }
        *item = Item::Value(Value::InlineTable(replacement));
    }
    let Some(child) = item.as_table_like_mut() else {
        bail!("Cannot edit {} inside a scalar value", full_path.join("."));
    };
    set_item(child, &path[1..], value, full_path)
}

fn normalize_alias(document: &mut DocumentMut, path: &[&str]) {
    let ["general", key, ..] = path else {
        return;
    };
    let alias = match *key {
        "input_cursor_active" => "input_corsur_active",
        "input_cursor_invalid" => "input_corsur_invalid",
        _ => return,
    };
    let Some(general) = document
        .get_mut("general")
        .and_then(Item::as_table_like_mut)
    else {
        return;
    };
    let alias_key = general.key(alias).cloned();
    if let Some(value) = general.remove(alias)
        && !general.contains_key(key)
    {
        general.insert(key, value);
        if let Some(old_key) = alias_key
            && let Some(mut new_key) = general.key_mut(key)
        {
            *new_key.leaf_decor_mut() = old_key.leaf_decor().clone();
            *new_key.dotted_decor_mut() = old_key.dotted_decor().clone();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(source: &str) -> (tempfile::TempDir, ConfigDocument) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        fs::write(&path, source).unwrap();
        let document = ConfigDocument::load(path).unwrap();
        (dir, document)
    }

    #[test]
    fn saving_one_preference_preserves_other_drafts_and_updates_the_baseline() {
        let (dir, mut document) = fixture(
            "# Keep my settings\ntheme = 'tui' # preferred theme\neditor_word_wrap = true\nfuture = 'keep'\n",
        );
        document
            .set(&["editor_word_wrap"], Some(false.into()))
            .unwrap();
        document
            .save_field_preserving_draft(&["theme"], Some("night".into()))
            .unwrap();

        let saved = ConfigDocument::load(dir.path().join("config.toml")).unwrap();
        assert_eq!(saved.value(&["theme"]), Some("night".into()));
        assert_eq!(saved.value(&["editor_word_wrap"]), Some(true.into()));
        assert!(saved.text().contains("# preferred theme"));
        assert!(saved.text().contains("future = 'keep'"));
        assert_eq!(document.value(&["editor_word_wrap"]), Some(false.into()));
        assert!(document.is_dirty());

        document.save().unwrap();
        let saved = ConfigDocument::load(dir.path().join("config.toml")).unwrap();
        assert_eq!(saved.value(&["theme"]), Some("night".into()));
        assert_eq!(saved.value(&["editor_word_wrap"]), Some(false.into()));
        assert!(!document.is_dirty());
    }

    #[test]
    fn saving_one_preference_to_a_missing_file_does_not_write_other_drafts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        let mut document = ConfigDocument::load(path.clone()).unwrap();
        document
            .set(&["auto_title"], Some("Draft title".into()))
            .unwrap();
        document
            .save_field_preserving_draft(&["theme"], Some("night".into()))
            .unwrap();
        let saved: toml::Value = toml::from_str(&fs::read_to_string(path).unwrap()).unwrap();
        assert_eq!(saved["theme"].as_str(), Some("night"));
        assert!(saved.get("auto_title").is_none());
        assert_eq!(document.value(&["auto_title"]), Some("Draft title".into()));
        assert!(document.is_dirty());
    }

    #[test]
    fn saving_one_preference_rejects_external_changes_without_losing_drafts() {
        let (_dir, mut document) = fixture("theme = 'tui'\neditor_word_wrap = true\n");
        document
            .set(&["editor_word_wrap"], Some(false.into()))
            .unwrap();
        let draft = document.text();
        fs::write(document.path(), "theme = 'external'\n").unwrap();
        assert!(
            document
                .save_field_preserving_draft(&["theme"], Some("night".into()))
                .is_err()
        );
        assert_eq!(document.text(), draft);
        assert_eq!(
            fs::read_to_string(document.path()).unwrap(),
            "theme = 'external'\n"
        );
    }

    #[test]
    fn invalid_partial_edit_cannot_change_the_file_or_draft() {
        let (_dir, mut document) = fixture("theme = 'tui'\n");
        document
            .set(&["auto_title"], Some("Draft title".into()))
            .unwrap();
        let draft = document.text();
        assert!(
            document
                .save_field_preserving_draft(&["theme", "name"], Some("night".into()))
                .is_err()
        );
        assert_eq!(document.text(), draft);
        assert_eq!(
            fs::read_to_string(document.path()).unwrap(),
            "theme = 'tui'\n"
        );
    }

    #[test]
    fn loading_and_discarding_a_missing_document_does_not_create_files() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing/config.toml");
        let mut document = ConfigDocument::load(path.clone()).unwrap();
        assert!(!document.is_dirty());
        document.save().unwrap();
        assert!(!path.parent().unwrap().exists());
        document
            .set(&["editor_word_wrap"], Some(true.into()))
            .unwrap();
        assert!(document.is_dirty());
        drop(document);
        assert!(!path.parent().unwrap().exists());
    }

    #[test]
    fn first_save_creates_parent_and_later_saves_update_the_baseline() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("missing/config.toml");
        let mut document = ConfigDocument::load(path.clone()).unwrap();
        for enabled in [false, true] {
            document
                .set(&["editor_word_wrap"], Some(enabled.into()))
                .unwrap();
            document.save().unwrap();
            assert!(!document.is_dirty());
            assert_eq!(fs::read_to_string(&path).unwrap(), document.text());
            assert_eq!(
                ConfigDocument::load(path.clone())
                    .unwrap()
                    .value(&["editor_word_wrap"]),
                Some(enabled.into())
            );
        }
    }

    #[test]
    fn changed_values_preserve_comments_unknown_fields_and_other_backends() {
        let source = "# personal settings\neditor_word_wrap  =  false # keep this note\nfuture = 'unchanged'\n\n[sqlite_backend]\nfile_path = '/tmp/existing.db' # keep database\n";
        let (_dir, mut document) = fixture(source);
        document
            .set(&["editor_word_wrap"], Some(true.into()))
            .unwrap();
        document.save().unwrap();
        let expected = source.replace("false # keep", "true # keep");
        assert_eq!(document.text(), expected);
        assert_eq!(fs::read_to_string(document.path()).unwrap(), expected);
    }

    #[test]
    fn toml_1_1_syntax_is_preserved_when_editing_another_field() {
        let source = "editor_word_wrap = false\nfuture = {\n  message = '\\e is literal',\n  escape = \"\\e\",\n}\n";
        let (_dir, mut document) = fixture(source);
        let _: toml::Value = toml::from_str(source).unwrap();
        assert_eq!(document.text(), source);
        document
            .set(&["editor_word_wrap"], Some(true.into()))
            .unwrap();
        document.save().unwrap();
        assert_eq!(document.text(), source.replacen("false", "true", 1));
        assert_eq!(document.value(&["future", "escape"]), Some("\u{1b}".into()));
    }

    #[test]
    fn assigning_the_existing_value_keeps_original_formatting_and_clean_state() {
        let source = "external_editor = 'vim' # editor\nscroll_per_page = 0x05\n";
        let (_dir, mut document) = fixture(source);
        document.set(&["scroll_per_page"], Some(5.into())).unwrap();
        document
            .set(&["external_editor", "command"], Some("vim".into()))
            .unwrap();
        document.set(&["missing", "field"], None).unwrap();
        assert!(!document.is_dirty());
        assert_eq!(document.text(), source);
    }

    #[test]
    fn editing_legacy_external_editor_keeps_command_and_comment() {
        let (_dir, mut document) = fixture("external_editor = 'vim -f' # editor choice\n");
        document
            .set(&["external_editor", "auto_save"], Some(true.into()))
            .unwrap();
        assert_eq!(
            document.value(&["external_editor", "command"]),
            Some("vim -f".into())
        );
        assert_eq!(
            document.value(&["external_editor", "auto_save"]),
            Some(true.into())
        );
        assert!(document.text().contains("# editor choice"));
        document.set(&["external_editor", "command"], None).unwrap();
        let settings: crate::settings::Settings = toml::from_str(&document.text()).unwrap();
        assert_eq!(settings.external_editor.command, None);
        assert!(settings.external_editor.auto_save);
        assert_eq!(settings.external_editor.temp_file_extension, "txt");
    }

    #[test]
    fn clearing_legacy_external_editor_command_preserves_defaults() {
        let (_dir, mut document) = fixture("external_editor = 'vim'\n");
        document.set(&["external_editor", "command"], None).unwrap();
        assert_eq!(document.value(&["external_editor", "command"]), None);
        let settings: crate::settings::Settings = toml::from_str(&document.text()).unwrap();
        assert_eq!(settings.external_editor.command, None);
        assert!(!settings.external_editor.auto_save);
        assert_eq!(settings.external_editor.temp_file_extension, "txt");
    }

    #[test]
    fn auto_title_transitions_between_literal_computed_and_disabled() {
        for source in [
            "auto_title = 'Daily' # title\n",
            "# title\n[auto_title] # header\nkind = 'date'\n",
            "auto_title = {kind = 'date'} # title\n",
        ] {
            let (_dir, mut document) = fixture(source);
            document
                .set(&["auto_title", "kind"], Some("date".into()))
                .unwrap();
            let _: crate::settings::Settings = toml::from_str(&document.text()).unwrap();
            assert_eq!(document.value(&["auto_title", "kind"]), Some("date".into()));
            document
                .set(
                    &["auto_title"],
                    Some("A literal 'title'\nwith lines".into()),
                )
                .unwrap();
            let _: crate::settings::Settings = toml::from_str(&document.text()).unwrap();
            assert!(document.text().contains("# title"));
            if source.contains("# header") {
                assert!(document.text().contains("# header"));
            }
            assert_eq!(
                document.value(&["auto_title"]),
                Some("A literal 'title'\nwith lines".into())
            );
            document.set(&["auto_title"], None).unwrap();
            assert_eq!(document.value(&["auto_title"]), None);
            let _: crate::settings::Settings = toml::from_str(&document.text()).unwrap();
        }
    }

    #[test]
    fn editing_theme_alias_normalizes_name_and_keeps_other_style_fields() {
        for (alias, canonical) in [
            ("input_corsur_active", "input_cursor_active"),
            ("input_corsur_invalid", "input_cursor_invalid"),
        ] {
            let source = format!(
                "# custom cursor\n[general.{alias}] # keep style\nfg = 'Red'\nbg = 'Blue'\nmodifiers = 'BOLD'\n\n[future]\nthing = 7\n"
            );
            let (_dir, mut document) = fixture(&source);
            assert_eq!(
                document.value(&["general", canonical, "bg"]),
                Some("Blue".into())
            );
            document
                .set(&["general", canonical, "fg"], Some("Green".into()))
                .unwrap();
            assert!(!document.text().contains(alias));
            assert!(document.text().contains("# custom cursor"));
            assert!(document.text().contains("# keep style"));
            assert_eq!(
                document.value(&["general", canonical, "fg"]),
                Some("Green".into())
            );
            assert_eq!(
                document.value(&["general", canonical, "bg"]),
                Some("Blue".into())
            );
            assert_eq!(document.value(&["future", "thing"]), Some(7.into()));
            let _: crate::app::ui::Styles = toml::from_str(&document.text()).unwrap();
        }
    }

    #[test]
    fn invalid_nested_edit_does_not_modify_the_draft() {
        let (_dir, mut document) = fixture("history_limit = 10 # preserve\n");
        let before = document.text();
        assert!(
            document
                .set(&["history_limit", "child"], Some(true.into()))
                .is_err()
        );
        assert!(document.set(&[], Some(true.into())).is_err());
        assert_eq!(document.text(), before);
        assert!(!document.is_dirty());
    }

    #[test]
    fn save_rejects_external_changes_even_without_local_edits() {
        for edit_draft in [false, true] {
            let (_dir, mut document) = fixture("history_limit = 10\n");
            if edit_draft {
                document.set(&["history_limit"], Some(20.into())).unwrap();
            }
            let before = document.text();
            fs::write(document.path(), "history_limit = 30\n").unwrap();
            assert!(document.save().is_err());
            assert_eq!(document.text(), before);
            assert_eq!(document.is_dirty(), edit_draft);
            assert_eq!(
                fs::read_to_string(document.path()).unwrap(),
                "history_limit = 30\n"
            );
            assert_eq!(
                document.baseline.source.as_deref(),
                Some("history_limit = 10\n")
            );
        }
    }

    #[test]
    fn save_rejects_deleted_files_and_new_files_created_externally() {
        let (_dir, mut document) = fixture("history_limit = 10\n");
        document.set(&["history_limit"], Some(20.into())).unwrap();
        fs::remove_file(document.path()).unwrap();
        assert!(document.save().is_err());
        assert!(!document.path().exists());
        assert!(document.is_dirty());

        let mut document = ConfigDocument::load(document.path().to_owned()).unwrap();
        document.set(&["history_limit"], Some(20.into())).unwrap();
        fs::write(document.path(), "history_limit = 30\n").unwrap();
        assert!(document.save().is_err());
        assert_eq!(
            fs::read_to_string(document.path()).unwrap(),
            "history_limit = 30\n"
        );
        assert!(document.baseline.source.is_none());
    }

    #[cfg(unix)]
    #[test]
    fn save_rejects_replaced_file_with_identical_contents() {
        let (dir, mut document) = fixture("history_limit = 10\n");
        let replacement = dir.path().join("replacement.toml");
        fs::write(&replacement, document.text()).unwrap();
        fs::rename(&replacement, document.path()).unwrap();
        assert!(document.save().is_err());
    }

    #[cfg(unix)]
    #[test]
    fn save_preserves_permissions_and_symlink() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let (dir, original) = fixture("history_limit = 10\n");
        fs::set_permissions(original.path(), Permissions::from_mode(0o640)).unwrap();
        let link = dir.path().join("linked.toml");
        symlink("config.toml", &link).unwrap();
        let mut document = ConfigDocument::load(link.clone()).unwrap();
        for limit in [20, 30] {
            document
                .set(&["history_limit"], Some(limit.into()))
                .unwrap();
            document.save().unwrap();
            assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
            assert_eq!(fs::read_link(&link).unwrap(), Path::new("config.toml"));
            assert_eq!(
                fs::metadata(original.path()).unwrap().permissions().mode() & 0o777,
                0o640
            );
            assert_eq!(
                fs::read_to_string(original.path()).unwrap(),
                document.text()
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn save_rejects_repointed_symlink_and_repointed_parent_directory() {
        use std::os::unix::fs::symlink;
        for parent_link in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            for name in ["first", "second"] {
                fs::create_dir(dir.path().join(name)).unwrap();
                fs::write(
                    dir.path().join(name).join("config.toml"),
                    "history_limit = 10\n",
                )
                .unwrap();
            }
            let link = dir.path().join("link");
            let (first, second, path) = if parent_link {
                (
                    Path::new("first"),
                    Path::new("second"),
                    link.join("config.toml"),
                )
            } else {
                (
                    Path::new("first/config.toml"),
                    Path::new("second/config.toml"),
                    link.clone(),
                )
            };
            symlink(first, &link).unwrap();
            let mut document = ConfigDocument::load(path).unwrap();
            document.set(&["history_limit"], Some(20.into())).unwrap();
            fs::remove_file(&link).unwrap();
            symlink(second, &link).unwrap();
            assert!(document.save().is_err());
            for name in ["first", "second"] {
                assert_eq!(
                    fs::read_to_string(dir.path().join(name).join("config.toml")).unwrap(),
                    "history_limit = 10\n"
                );
            }
            assert!(document.is_dirty());
        }
    }
}

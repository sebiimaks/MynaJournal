use toml::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Tab {
    General,
    Entries,
    Editor,
    Security,
    Storage,
    Appearance,
    Layout,
    Context,
}

impl Tab {
    pub const ALL: [Self; 8] = [
        Self::General,
        Self::Entries,
        Self::Editor,
        Self::Storage,
        Self::Appearance,
        Self::Layout,
        Self::Security,
        Self::Context,
    ];

    pub const fn title(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Entries => "Entries",
            Self::Editor => "Editor",
            Self::Security => "Security",
            Self::Storage => "Storage",
            Self::Appearance => "Appearance",
            Self::Layout => "Layout",
            Self::Context => "Context",
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum Kind {
    Bool,
    Choice(Vec<&'static str>),
    Number { min: u64, max: u64 },
    Text { allow_empty: bool },
    Extension,
    TitleMode,
    TitleLiteral,
    Color { optional: bool },
    Modifiers,
    TagPalette,
    Divider,
}

#[derive(Debug, Clone)]
pub(super) struct Field {
    pub tab: Tab,
    pub label: String,
    pub path: Vec<String>,
    pub kind: Kind,
    pub restart: bool,
    pub hint: String,
}

impl Field {
    fn new(tab: Tab, path: &[&str], label: &str, kind: Kind, hint: &str) -> Self {
        Self {
            tab,
            label: label.into(),
            path: path.iter().map(|key| (*key).into()).collect(),
            kind,
            restart: false,
            hint: hint.into(),
        }
    }

    fn restart(mut self) -> Self {
        self.restart = true;
        self
    }

    pub fn keys(&self) -> Vec<&str> {
        self.path.iter().map(String::as_str).collect()
    }
}

pub(super) fn fields(default_styles: &Value) -> Vec<Field> {
    use Kind::*;
    use Tab::*;
    let number = || Number {
        min: 0,
        max: (i64::MAX as usize) as u64,
    };
    let path = || Text { allow_empty: false };
    let mut fields = vec![
        Field::new(Security, &["security", "auto_lock_seconds"], "Lock after idle seconds", Number { min: 0, max: 86400 }, "Lock protected notes after inactivity. Default 300 seconds; zero disables the timer. Unsaved text is sealed in memory until you unlock again."),
        Field::new(Security, &["security", "lock_on_leave"], "Lock when leaving note", Bool, "Forget a protected note's password when selecting another note. Titles and dates remain visible."),
        Field::new(General, &["colored_tags"], "Colored tags", Bool, "Use a distinct color for each tag."),
        Field::new(General, &["datum_visibility"], "Entry dates", Choice(vec!["show", "hide", "empty_line"]), "Show dates, hide them, or reserve an empty line."),
        Field::new(General, &["scroll_per_page"], "Entries per page step", number(), "Number of entries moved by list page navigation. Zero disables list page movement."),
        Field::new(General, &["history_limit"], "Journal undo limit", number(), "Maximum saved journal changes in each undo/redo stack. Zero disables journal undo. Editor text undo is separate. Restart required.").restart(),
        Field::new(Entries, &["default_journal_priority"], "New-entry priority", Number { min: 0, max: u32::MAX as u64 }, "Default priority for new entries; use default to leave it unset."),
        Field::new(Entries, &["auto_title"], "New-entry title mode", TitleMode, "None, current date, or a literal title. Applies to new entries."),
        Field::new(Entries, &["auto_title"], "Literal title", TitleLiteral, "Editing this value selects literal-title mode. Empty titles are allowed."),
        Field::new(Entries, &["export", "default_path"], "Export destination", path(), "Default file or directory for the next export dialog; use default to leave unset."),
        Field::new(Entries, &["export", "show_confirmation"], "Export confirmation", Bool, "Show a message after successful export."),
        Field::new(Editor, &["editor_word_wrap"], "Word wrap", Bool, "Wrap the journal visually without inserting newlines."),
        Field::new(Editor, &["sync_os_clipboard"], "Clipboard synchronization", Bool, "Use the operating-system clipboard during editor clipboard actions."),
        Field::new(Editor, &["external_editor", "command"], "External editor command", path(), "Command used on the next external edit. Default checks git, VISUAL, EDITOR, then vi. Saving does not run it."),
        Field::new(Editor, &["external_editor", "auto_save"], "External editor auto-save", Bool, "Save imported text automatically after external editing."),
        Field::new(Editor, &["external_editor", "temp_file_extension"], "External editor extension", Extension, "Temporary filename extension, such as txt or md; cannot contain directory separators."),
    ];
    let backends = vec![
        #[cfg(feature = "json")]
        "Json",
        #[cfg(feature = "sqlite")]
        "Sqlite",
        #[cfg(feature = "vjournal")]
        "Vjournal",
        #[cfg(feature = "file")]
        "File",
    ];
    let availability = [
        ("Json", cfg!(feature = "json")),
        ("Sqlite", cfg!(feature = "sqlite")),
        ("Vjournal", cfg!(feature = "vjournal")),
        ("File", cfg!(feature = "file")),
    ]
    .into_iter()
    .map(|(name, enabled)| {
        format!(
            "{name}: {}",
            if enabled {
                "available"
            } else {
                "unavailable in this build"
            }
        )
    })
    .collect::<Vec<_>>()
    .join("; ");
    fields.push(
        Field::new(
            Storage,
            &["backend_type"],
            "Backend",
            Choice(backends),
            &format!("{availability}. Restart required."),
        )
        .restart(),
    );
    #[cfg(feature = "json")]
    fields.push(
        Field::new(
            Storage,
            &["json_backend", "file_path"],
            "JSON file",
            path(),
            "JSON journal file. Restart required.",
        )
        .restart(),
    );
    #[cfg(feature = "sqlite")]
    fields.push(
        Field::new(
            Storage,
            &["sqlite_backend", "file_path"],
            "SQLite file",
            path(),
            "SQLite journal database. Restart required.",
        )
        .restart(),
    );
    #[cfg(feature = "vjournal")]
    fields.push(
        Field::new(
            Storage,
            &["vjournal_backend", "directory"],
            "VJOURNAL directory",
            path(),
            "Local VJOURNAL directory. Restart required.",
        )
        .restart(),
    );
    #[cfg(feature = "file")]
    fields.extend([
        Field::new(
            Storage,
            &["file_backend", "directory"],
            "File directory",
            path(),
            "Local directory containing one plain-text or Markdown file per entry. Restart required.",
        )
        .restart(),
        Field::new(
            Storage,
            &["file_backend", "format"],
            "File format",
            Choice(vec!["plain", "markdown"]),
            "New files: plain has no extension; markdown uses .md. Both are always read; existing filenames stay unchanged. Restart required.",
        )
        .restart(),
    ]);
    fields.push(Field::new(Storage, &["app_state_dir"], "App-state directory", path(), "Directory for pane size, fullscreen and sorting state. Current-session saves keep their current destination. Restart required.").restart());

    for group in ["general", "journals_list", "editor"] {
        if let Some(table) = default_styles.get(group).and_then(Value::as_table) {
            for name in table.keys() {
                for property in ["fg", "bg", "modifiers", "underline_color"] {
                    let kind = if property == "modifiers" {
                        Modifiers
                    } else {
                        Color { optional: true }
                    };
                    let hint = if property == "modifiers" {
                        "Names separated by |, e.g. BOLD | UNDERLINED. Empty means no modifiers; d restores this property's preset."
                    } else {
                        "Color name, #RRGGBB, palette index 0-255, or Reset. 'inherit' removes this color; d restores this property's preset."
                    };
                    fields.push(Field::new(
                        Appearance,
                        &[group, name, property],
                        &format!("{group}.{name}.{property}"),
                        kind,
                        hint,
                    ));
                }
            }
        }
    }
    for name in ["error", "warning", "info", "question"] {
        fields.push(Field::new(
            Appearance,
            &["msgbox", name],
            &format!("msgbox.{name}"),
            Color { optional: false },
            "Color name, #RRGGBB, palette index 0-255, or Reset. d restores the preset.",
        ));
    }
    fields.push(Field::new(
        Appearance,
        &["tags", "palette"],
        "tags.palette",
        TagPalette,
        "Tag colors as FG on BG pairs separated by commas, e.g. Black on LightCyan, White on #3A5F8F. Tags take them in alphabetical order; tags beyond the list use Reset. d restores the preset.",
    ));
    fields.push(Field::new(
        Layout,
        &[],
        "Entries-pane width (%)",
        Divider,
        "10-60 percent. Saved separately to app state; fullscreen is unchanged.",
    ));
    fields
}

use std::fs;

use ratatui::{Terminal, backend::TestBackend};

use super::*;

fn popup(settings: &str, themes: &str) -> (tempfile::TempDir, ConfigPopup) {
    let directory = tempfile::tempdir().unwrap();
    let settings_path = directory.path().join("config.toml");
    let themes_path = directory.path().join("themes/theme_tui.toml");
    fs::create_dir(themes_path.parent().unwrap()).unwrap();
    fs::write(&settings_path, settings).unwrap();
    fs::write(&themes_path, themes).unwrap();
    let popup = ConfigPopup::new(
        ConfigDocument::load(settings_path).unwrap(),
        ThemeStore::new(Some(&directory.path().to_path_buf())).unwrap(),
        &toml::from_str(settings).unwrap(),
        &Styles::deserialize(themes).unwrap(),
        Some("tui"),
        37,
    )
    .unwrap();
    (directory, popup)
}

fn key(popup: &mut ConfigPopup, code: KeyCode) -> ConfigAction {
    popup.handle_input(&Input::new(code, KeyModifiers::NONE))
}

fn select(popup: &mut ConfigPopup, tab: Tab, path: &[&str]) {
    if tab == Tab::Appearance && popup.themes_document.is_none() {
        popup.edit_selected_theme();
    }
    popup.tab = Tab::ALL
        .iter()
        .position(|candidate| *candidate == tab)
        .unwrap();
    let selected = popup
        .tab_fields()
        .position(|field| field.keys() == path)
        .unwrap();
    popup.selected[popup.tab] = selected;
}

#[test]
fn toggles_are_staged_and_closing_requires_explicit_discard() {
    let (directory, mut popup) = popup("# kept\ncolored_tags = true\n", "");
    assert_eq!(key(&mut popup, KeyCode::Enter), ConfigAction::Keep);
    assert_eq!(
        popup.settings_document.value(&["colored_tags"]),
        Some(Value::Boolean(false))
    );
    assert!(popup.is_dirty());
    assert!(
        fs::read_to_string(directory.path().join("config.toml"))
            .unwrap()
            .contains("true")
    );
    assert_eq!(key(&mut popup, KeyCode::Esc), ConfigAction::Keep);
    assert!(popup.confirm_close);
    assert_eq!(key(&mut popup, KeyCode::Char('n')), ConfigAction::Keep);
    assert!(!popup.confirm_close);
    assert!(popup.is_dirty());
    key(&mut popup, KeyCode::Esc);
    assert_eq!(key(&mut popup, KeyCode::Char('y')), ConfigAction::Close);
}

#[test]
fn invalid_field_keeps_text_and_previous_document_value() {
    let (_, mut popup) = popup("scroll_per_page = 5\n", "");
    select(&mut popup, Tab::General, &["scroll_per_page"]);
    key(&mut popup, KeyCode::Enter);
    popup.editor = Some(TextArea::new(vec!["-1".into()]));
    key(&mut popup, KeyCode::Enter);
    assert!(popup.error);
    assert_eq!(popup.editor.as_ref().unwrap().lines(), &["-1"]);
    assert_eq!(
        popup.settings_document.value(&["scroll_per_page"]),
        Some(Value::Integer(5))
    );
    assert!(!popup.is_dirty());
    popup.editor = Some(TextArea::new(vec!["9".into()]));
    assert_eq!(
        popup.handle_input(&Input::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
        ConfigAction::SaveSettings
    );
    assert!(popup.editor.is_none());
    assert_eq!(
        popup.settings_document.value(&["scroll_per_page"]),
        Some(Value::Integer(9))
    );
}

#[test]
fn text_editing_keeps_popup_shortcuts_literal_and_escape_cancels_only_field() {
    let (_, mut popup) = popup("", "");
    select(&mut popup, Tab::Entries, &["auto_title"]);
    popup.selected[popup.tab] += 1;
    key(&mut popup, KeyCode::Enter);
    for ch in "q!d".chars() {
        assert_eq!(key(&mut popup, KeyCode::Char(ch)), ConfigAction::Keep);
    }
    assert_eq!(popup.editor.as_ref().unwrap().lines(), &["q!d"]);
    key(&mut popup, KeyCode::Esc);
    assert!(popup.editor.is_none());
    assert!(!popup.is_dirty());
    assert!(!popup.confirm_close);
}

#[test]
fn separate_documents_and_layout_return_separate_save_actions() {
    let (_, mut popup) = popup("", "");
    let save = Input::new(KeyCode::Char('s'), KeyModifiers::CONTROL);
    assert_eq!(popup.handle_input(&save), ConfigAction::SaveSettings);
    for _ in 0..4 {
        key(&mut popup, KeyCode::Tab);
    }
    assert_eq!(popup.handle_input(&save), ConfigAction::Keep);
    key(&mut popup, KeyCode::Char('e'));
    assert_eq!(popup.handle_input(&save), ConfigAction::SaveAppearance);
    key(&mut popup, KeyCode::Tab);
    assert_eq!(popup.handle_input(&save), ConfigAction::SaveLayout(37));
    key(&mut popup, KeyCode::Char('d'));
    assert_eq!(popup.handle_input(&save), ConfigAction::SaveLayout(30));
    assert!(!popup.settings_document.is_dirty());
    assert!(!popup.theme_dirty());
    popup.mark_layout_saved(30);
    assert!(!popup.is_dirty());
}

#[test]
fn editing_inherited_color_keeps_the_other_preset_properties() {
    let (_, mut popup) = popup("", "");
    select(
        &mut popup,
        Tab::Appearance,
        &["editor", "cursor_normal", "fg"],
    );
    let field = popup.current_field().clone();
    let original = nested(&popup.themes, &["editor", "cursor_normal"])
        .unwrap()
        .clone();
    popup
        .apply_value(&field, Some(Value::String("Red".into())))
        .unwrap();
    let actual = Styles::deserialize(&popup.themes_document.as_ref().unwrap().text()).unwrap();
    assert_eq!(actual.editor.cursor_normal.fg, Some(Color::Red));
    assert_eq!(
        Value::try_from(actual.editor.cursor_normal)
            .unwrap()
            .get("bg"),
        original.get("bg")
    );
    assert_eq!(
        Value::try_from(actual.editor.cursor_normal)
            .unwrap()
            .get("modifiers"),
        original.get("modifiers")
    );
    assert!(!popup.settings_document.is_dirty());
    popup.apply_value(&field, None).unwrap();
    assert_eq!(
        Styles::deserialize(&popup.themes_document.as_ref().unwrap().text())
            .unwrap()
            .editor
            .cursor_normal
            .fg,
        None
    );
    popup.restore_default();
    assert_eq!(
        Styles::deserialize(&popup.themes_document.as_ref().unwrap().text())
            .unwrap()
            .editor
            .cursor_normal,
        Styles::default().editor.cursor_normal
    );
}

#[test]
fn legacy_editor_command_survives_editing_another_editor_option() {
    let (_, mut popup) = popup("external_editor = 'nvim -f'\n", "");
    select(&mut popup, Tab::Editor, &["external_editor", "auto_save"]);
    key(&mut popup, KeyCode::Enter);
    let settings: Settings = toml::from_str(&popup.settings_document.text()).unwrap();
    assert_eq!(settings.external_editor.command.as_deref(), Some("nvim -f"));
    assert!(settings.external_editor.auto_save);
}

#[test]
fn use_default_removes_setting_override_and_marks_restart_difference() {
    let (_, mut popup) = popup("history_limit = 25\n", "");
    select(&mut popup, Tab::General, &["history_limit"]);
    key(&mut popup, KeyCode::Char('d'));
    assert!(popup.settings_document.value(&["history_limit"]).is_none());
    let field = popup.current_field();
    assert!(field.restart);
    assert_eq!(popup.value(field), Some(Value::Integer(10)));
    assert_eq!(popup.active_value(field), Some(Value::Integer(25)));
}

#[test]
fn appearance_schema_exposes_every_style_component_and_message_color() {
    let (_, popup) = popup("", "");
    let appearance: Vec<_> = popup
        .fields
        .iter()
        .filter(|field| field.tab == Tab::Appearance)
        .collect();
    assert_eq!(appearance.len(), (7 + 10 + 8) * 4 + 4 + 1);
    for group in ["general", "journals_list", "editor"] {
        for name in popup.default_themes[group].as_table().unwrap().keys() {
            for property in ["fg", "bg", "modifiers", "underline_color"] {
                assert!(
                    appearance
                        .iter()
                        .any(|field| field.keys() == [group, name, property])
                );
            }
        }
    }
    assert!(
        appearance
            .iter()
            .any(|field| field.keys() == ["tags", "palette"])
    );
}

#[test]
fn tag_palette_field_edits_all_pairs_as_one_line() {
    let (_directory, mut popup) = popup("", &Styles::serialize_default().unwrap());
    select(&mut popup, Tab::Appearance, &["tags", "palette"]);
    let field = popup.current_field().clone();
    assert_eq!(
        editable_value(&field.kind, popup.value(&field).as_ref()),
        "Black on LightCyan, Black on LightRed, Black on LightGreen, Reset on DarkGray, \
         Black on LightYellow, Reset on Red, Yellow on Blue, Red on Cyan, Black on LightMagenta"
    );

    let value = parse_input(&field.kind, " white ON #3a5f8f,Black on light yellow, ").unwrap();
    popup.apply_value(&field, value).unwrap();
    let text = popup.themes_document.as_ref().unwrap().text();
    assert!(!text.contains("[[tags.palette]]"));
    assert_eq!(
        Styles::deserialize(&text).unwrap().tags.palette,
        [
            TagColors::new(Color::White, Color::Rgb(0x3A, 0x5F, 0x8F)),
            TagColors::new(Color::Black, Color::LightYellow),
        ]
    );
    assert_eq!(
        display_value(&field.kind, popup.value(&field).as_ref()),
        "White on #3A5F8F, Black on LightYellow"
    );

    let empty = parse_input(&field.kind, "").unwrap();
    popup.apply_value(&field, empty).unwrap();
    assert_eq!(
        display_value(&field.kind, popup.value(&field).as_ref()),
        "(none)"
    );

    popup.restore_default();
    assert_eq!(
        Styles::deserialize(&popup.themes_document.as_ref().unwrap().text())
            .unwrap()
            .tags,
        Styles::default().tags
    );
}

#[test]
fn tag_palette_field_rejects_pairs_without_two_colors() {
    for input in [
        "Black",
        "Black on",
        "on Blue",
        "Black Blue",
        "Black on Blue on Red",
        "Black on not-a-color",
        "Black on Blue; White on Red",
        "inherit on Blue",
    ] {
        assert!(
            parse_input(&Kind::TagPalette, input).is_err(),
            "accepted {input:?}"
        );
    }
}

#[test]
fn typed_fields_reject_invalid_ranges_colors_modifiers_and_extensions() {
    for value in ["-1", "18446744073709551616", "1.5", ""] {
        assert!(parse_input(&Kind::Number { min: 0, max: 100 }, value).is_err());
    }
    assert!(parse_input(&Kind::Divider, "9").is_err());
    assert!(parse_input(&Kind::Divider, "61").is_err());
    assert!(parse_input(&Kind::Color { optional: false }, "not-a-color").is_err());
    assert!(parse_input(&Kind::Modifiers, "BOLD | NOT_A_MODIFIER").is_err());
    assert!(parse_input(&Kind::Extension, "../md").is_err());
    assert!(parse_input(&Kind::Extension, "a\\b").is_err());
    assert!(parse_input(&Kind::Extension, "").is_ok());
    assert_eq!(
        parse_input(&Kind::Color { optional: true }, "inherit").unwrap(),
        None
    );
    assert_eq!(
        parse_input(&Kind::Color { optional: true }, "Reset").unwrap(),
        Some(Value::String("Reset".into()))
    );
    assert!(parse_input(&Kind::Modifiers, "bold | underlined").is_ok());
}

#[test]
fn renderer_bounds_all_tabs_and_edit_states_on_small_terminals() {
    let (_, mut popup) = popup("", "");
    for (width, height) in [(1, 1), (10, 4), (20, 8), (40, 12), (80, 24), (120, 40)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        for tab in 0..Tab::ALL.len() {
            popup.tab = tab;
            terminal
                .draw(|frame| popup.render_widget(frame, frame.area(), &Styles::default()))
                .unwrap();
            popup.editor = Some(TextArea::new(vec!["long text q! 界".repeat(10)]));
            popup.show_error("A validation error that must remain inside the popup.".into());
            terminal
                .draw(|frame| popup.render_widget(frame, frame.area(), &Styles::default()))
                .unwrap();
            popup.editor = None;
        }
    }
}

#[test]
fn existing_control_characters_are_not_sent_to_the_field_editor() {
    let (_, mut popup) = popup("auto_title = \"unsafe\\r\\u001b[31m\"\n", "");
    select(&mut popup, Tab::Entries, &["auto_title"]);
    popup.selected[popup.tab] += 1;
    let before = popup.settings_document.text();
    key(&mut popup, KeyCode::Enter);
    assert!(popup.editor.is_none());
    assert!(popup.error);
    assert_eq!(popup.settings_document.text(), before);
    key(&mut popup, KeyCode::Char('d'));
    assert!(popup.settings_document.value(&["auto_title"]).is_none());
}

#[test]
fn context_is_scrollable_and_keeps_metadata_accessible_on_small_screens() {
    let (_, mut popup) = popup("", "");
    popup.set_launch_details(vec![
        "Log file: /a/very/long/path/to/the/log".into(),
        "Startup verbosity: Trace".into(),
    ]);
    popup.tab = Tab::ALL.len() - 1;
    let mut terminal = Terminal::new(TestBackend::new(40, 12)).unwrap();
    key(&mut popup, KeyCode::End);
    terminal
        .draw(|frame| popup.render_widget(frame, frame.area(), &Styles::default()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    let rendered: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
    assert!(rendered.contains("Startup verbosity: Trace"));
    assert!(rendered.contains("Context"));
    assert!(popup.context_scroll > 0);
    key(&mut popup, KeyCode::Home);
    assert_eq!(popup.context_scroll, 0);
    assert_eq!(
        popup.handle_input(&Input::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
        ConfigAction::Keep
    );
}

#[test]
fn context_footer_keeps_navigation_and_discard_choices_visible_on_narrow_screens() {
    for width in [25, 30] {
        let (_, mut popup) = popup("", "");
        key(&mut popup, KeyCode::Enter);
        key(&mut popup, KeyCode::BackTab);
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
        terminal
            .draw(|frame| popup.render_widget(frame, frame.area(), &Styles::default()))
            .unwrap();
        let rendered: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        for hint in ["↑↓ Scroll", "Tab Next", "Esc Close"] {
            assert!(rendered.contains(hint), "missing {hint:?} at width {width}");
        }
        assert_eq!(key(&mut popup, KeyCode::Esc), ConfigAction::Keep);
        terminal
            .draw(|frame| popup.render_widget(frame, frame.area(), &Styles::default()))
            .unwrap();
        let rendered: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(rendered.contains("Discard? y/n"), "width {width}");
        assert_eq!(key(&mut popup, KeyCode::Char('n')), ConfigAction::Keep);
        assert!(!popup.confirm_close);
        assert!(popup.is_dirty());
    }
}

#[test]
fn storage_path_checks_existing_types_without_creating_new_paths() {
    let (directory, mut popup) = popup("", "");
    select(&mut popup, Tab::Storage, &["app_state_dir"]);
    let field = popup.current_field().clone();
    let existing_file = directory.path().join("existing");
    fs::write(&existing_file, "keep").unwrap();
    assert!(
        popup
            .apply_value(
                &field,
                Some(Value::String(existing_file.display().to_string()))
            )
            .is_err()
    );
    assert!(!popup.is_dirty());
    let missing = directory.path().join("new-directory");
    popup
        .apply_value(&field, Some(Value::String(missing.display().to_string())))
        .unwrap();
    assert!(!missing.exists());
    #[cfg(feature = "json")]
    {
        select(&mut popup, Tab::Storage, &["json_backend", "file_path"]);
        let field = popup.current_field().clone();
        assert!(
            popup
                .apply_value(
                    &field,
                    Some(Value::String(directory.path().display().to_string()))
                )
                .is_err()
        );
    }
}

#[cfg(feature = "file")]
#[test]
fn file_storage_options_validate_paths_and_stage_restart_only_changes() {
    let (directory, mut popup) = popup("", "");
    select(&mut popup, Tab::Storage, &["file_backend", "directory"]);
    let field = popup.current_field().clone();
    assert!(field.restart);
    let existing_file = directory.path().join("existing");
    fs::write(&existing_file, "keep").unwrap();
    assert!(
        popup
            .apply_value(&field, Some(existing_file.display().to_string().into()))
            .is_err()
    );
    assert!(!popup.is_dirty());
    let missing = directory.path().join("new-entries");
    popup
        .apply_value(&field, Some(missing.display().to_string().into()))
        .unwrap();
    assert!(!missing.exists());

    select(&mut popup, Tab::Storage, &["file_backend", "format"]);
    assert!(popup.current_field().restart);
    assert_eq!(popup.value(popup.current_field()), Some("plain".into()));
    key(&mut popup, KeyCode::Enter);
    assert_eq!(popup.value(popup.current_field()), Some("markdown".into()));
    assert_eq!(
        popup.active_value(popup.current_field()),
        Some("plain".into())
    );
    let settings: Settings = toml::from_str(&popup.settings_document.text()).unwrap();
    assert_eq!(settings.file_backend.directory, Some(missing));
    assert_eq!(settings.file_backend.format, backend::FileFormat::Markdown);
    key(&mut popup, KeyCode::Char('d'));
    assert_eq!(popup.value(popup.current_field()), Some("plain".into()));
    assert!(
        popup
            .settings_document
            .value(&["file_backend", "format"])
            .is_none()
    );
}

#[test]
fn backend_choice_only_contains_compiled_backends() {
    let (_, mut popup) = popup("", "");
    select(&mut popup, Tab::Storage, &["backend_type"]);
    let Kind::Choice(choices) = &popup.current_field().kind else {
        panic!("backend must be a choice");
    };
    for (name, available) in [
        ("Json", cfg!(feature = "json")),
        ("Sqlite", cfg!(feature = "sqlite")),
        ("Vjournal", cfg!(feature = "vjournal")),
        ("File", cfg!(feature = "file")),
    ] {
        assert_eq!(choices.contains(&name), available, "{name}");
    }
}

fn appearance(popup: &mut ConfigPopup) {
    popup.tab = Tab::ALL
        .iter()
        .position(|tab| *tab == Tab::Appearance)
        .unwrap();
}

fn lazy_popup() -> (tempfile::TempDir, ConfigPopup) {
    let directory = tempfile::tempdir().unwrap();
    let popup = ConfigPopup::new(
        ConfigDocument::load(directory.path().join("config.toml")).unwrap(),
        ThemeStore::from_directory(directory.path().join("themes")).unwrap(),
        &Settings::default(),
        &Styles::default(),
        Some("tui"),
        37,
    )
    .unwrap();
    (directory, popup)
}

#[test]
fn browsing_activating_and_cancelling_builtin_theme_creates_no_files() {
    let (directory, mut popup) = lazy_popup();
    appearance(&mut popup);
    assert_eq!(popup.theme_catalog.len(), 1);
    assert_eq!(popup.theme_catalog[0].label, "Tui (default)");
    assert_eq!(key(&mut popup, KeyCode::Enter), ConfigAction::ActivateTheme);
    let (id, styles) = popup.selected_theme().unwrap();
    assert_eq!(id, "tui");
    assert_eq!(
        Value::try_from(styles).unwrap(),
        Value::try_from(Styles::default()).unwrap()
    );
    key(&mut popup, KeyCode::Char('e'));
    assert_eq!(popup.edited_theme_id(), Some("tui"));
    assert!(!popup.theme_dirty());
    assert_eq!(key(&mut popup, KeyCode::Esc), ConfigAction::Keep);
    assert!(popup.themes_document.is_none());
    assert_eq!(key(&mut popup, KeyCode::Esc), ConfigAction::Close);
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn returning_to_list_discards_only_theme_edits_after_confirmation() {
    let (_, mut popup) = lazy_popup();
    key(&mut popup, KeyCode::Enter);
    let settings_draft = popup.settings_document.text();
    popup.divider = 45;
    appearance(&mut popup);
    key(&mut popup, KeyCode::Char('e'));
    select(&mut popup, Tab::Appearance, &["msgbox", "error"]);
    key(&mut popup, KeyCode::Enter);
    popup.editor = Some(TextArea::new(vec!["Green".into()]));
    key(&mut popup, KeyCode::Enter);
    assert!(popup.theme_dirty());
    key(&mut popup, KeyCode::Esc);
    assert!(popup.confirm_theme_close);
    assert!(!popup.confirm_close);
    key(&mut popup, KeyCode::Char('n'));
    assert!(popup.themes_document.is_some());
    key(&mut popup, KeyCode::Esc);
    key(&mut popup, KeyCode::Char('y'));
    assert!(popup.themes_document.is_none());
    assert!(!popup.theme_dirty());
    assert!(popup.settings_document.is_dirty());
    assert_eq!(popup.settings_document.text(), settings_draft);
    assert_eq!(popup.divider, 45);
}

#[test]
fn theme_field_cancel_keeps_editor_and_other_theme_drafts() {
    let (_, mut popup) = lazy_popup();
    appearance(&mut popup);
    key(&mut popup, KeyCode::Char('e'));
    select(&mut popup, Tab::Appearance, &["msgbox", "error"]);
    let field = popup.current_field().clone();
    popup
        .apply_value(&field, Some(Value::String("Green".into())))
        .unwrap();
    let draft = popup.themes_document.as_ref().unwrap().text();
    key(&mut popup, KeyCode::Enter);
    popup.editor = Some(TextArea::new(vec!["invalid".into()]));
    key(&mut popup, KeyCode::Esc);
    assert!(popup.editor.is_none());
    assert!(!popup.confirm_theme_close);
    assert_eq!(popup.themes_document.as_ref().unwrap().text(), draft);
    key(&mut popup, KeyCode::Tab);
    key(&mut popup, KeyCode::BackTab);
    assert_eq!(popup.current_field().keys(), ["msgbox", "error"]);
    assert_eq!(popup.themes_document.as_ref().unwrap().text(), draft);
}

#[test]
fn new_theme_validates_name_and_stays_lazy_until_saved() {
    let (directory, mut popup) = lazy_popup();
    appearance(&mut popup);
    key(&mut popup, KeyCode::Char('n'));
    popup.editor = Some(TextArea::new(vec!["../bad".into()]));
    key(&mut popup, KeyCode::Enter);
    assert!(popup.error);
    assert!(popup.creating_theme);
    assert!(popup.themes_document.is_none());
    popup.editor = Some(TextArea::new(vec!["Night Sky".into()]));
    key(&mut popup, KeyCode::Enter);
    assert!(!popup.creating_theme);
    assert_eq!(popup.edited_theme_id(), Some("night_sky"));
    assert!(popup.theme_dirty());
    assert!(!popup.theme_is_active());
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
    key(&mut popup, KeyCode::Esc);
    key(&mut popup, KeyCode::Char('y'));
    assert_eq!(popup.theme_catalog.len(), 1);
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 0);
}

#[test]
fn saving_duplicate_refreshes_catalog_without_activating_it() {
    let (directory, mut popup) = lazy_popup();
    appearance(&mut popup);
    key(&mut popup, KeyCode::Char('n'));
    popup.editor = Some(TextArea::new(vec!["Night Sky".into()]));
    assert_eq!(
        popup.handle_input(&Input::new(KeyCode::Char('s'), KeyModifiers::CONTROL)),
        ConfigAction::SaveAppearance
    );
    select(&mut popup, Tab::Appearance, &["msgbox", "error"]);
    let field = popup.current_field().clone();
    popup
        .apply_value(&field, Some(Value::String("Green".into())))
        .unwrap();
    let styles = Styles::deserialize(&popup.themes_document.as_ref().unwrap().text()).unwrap();
    popup.themes_document_mut().unwrap().save().unwrap();
    popup.mark_appearance_saved(&styles);
    assert!(!popup.theme_dirty());
    assert_eq!(popup.active_theme_id.as_deref(), Some("tui"));
    assert_eq!(popup.saved_themes, Value::try_from(&styles).unwrap());
    assert!(
        directory
            .path()
            .join("themes/theme_night_sky.toml")
            .is_file()
    );
    assert!(!directory.path().join("themes/theme_tui.toml").exists());
    key(&mut popup, KeyCode::Esc);
    assert_eq!(popup.theme_catalog[popup.theme_selected].id, "night_sky");
    assert_eq!(popup.theme_catalog[popup.theme_selected].label, "Night Sky");
    assert_eq!(
        key(&mut popup, KeyCode::Char('a')),
        ConfigAction::ActivateTheme
    );
    let (id, _) = popup.selected_theme().unwrap();
    popup.mark_theme_activated(&id);
    assert_eq!(popup.active_theme_id.as_deref(), Some("night_sky"));
}

#[test]
fn saving_active_theme_updates_its_saved_styles() {
    let (_, mut popup) = lazy_popup();
    appearance(&mut popup);
    key(&mut popup, KeyCode::Char('e'));
    select(&mut popup, Tab::Appearance, &["msgbox", "error"]);
    let field = popup.current_field().clone();
    popup
        .apply_value(&field, Some(Value::String("Green".into())))
        .unwrap();
    let styles = Styles::deserialize(&popup.themes_document.as_ref().unwrap().text()).unwrap();
    popup.themes_document_mut().unwrap().save().unwrap();
    popup.mark_appearance_saved(&styles);
    assert!(popup.theme_is_active());
    assert_eq!(popup.saved_themes, Value::try_from(&styles).unwrap());
}

#[test]
fn malformed_theme_is_listed_but_cannot_be_edited_or_activated() {
    let directory = tempfile::tempdir().unwrap();
    fs::create_dir(directory.path().join("themes")).unwrap();
    fs::write(directory.path().join("themes/theme_tui.toml"), "[broken").unwrap();
    fs::write(
        directory.path().join("themes/theme_night.toml"),
        "[msgbox]\nerror = 'Green'\n",
    )
    .unwrap();
    let mut popup = ConfigPopup::new(
        ConfigDocument::load(directory.path().join("config.toml")).unwrap(),
        ThemeStore::from_directory(directory.path().join("themes")).unwrap(),
        &Settings::default(),
        &Styles::default(),
        Some("tui"),
        37,
    )
    .unwrap();
    appearance(&mut popup);
    assert!(popup.theme_catalog[0].error.is_some());
    assert!(popup.selected_theme().is_err());
    key(&mut popup, KeyCode::Char('e'));
    assert!(popup.error);
    assert!(popup.themes_document.is_none());
    key(&mut popup, KeyCode::Char('n'));
    assert!(popup.editor.is_none());
    key(&mut popup, KeyCode::Down);
    assert_eq!(popup.selected_theme().unwrap().0, "night");
    key(&mut popup, KeyCode::Char('e'));
    assert_eq!(popup.edited_theme_id(), Some("night"));
}

#[test]
fn theme_list_and_editor_show_identity_navigation_and_discard_prompt() {
    let (_, mut popup) = lazy_popup();
    appearance(&mut popup);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    let render = |popup: &mut ConfigPopup, terminal: &mut Terminal<TestBackend>| {
        terminal
            .draw(|frame| popup.render_widget(frame, frame.area(), &Styles::default()))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
    };
    let text = render(&mut popup, &mut terminal);
    for expected in [
        "Tui (default)",
        "Active",
        "activate",
        "edit",
        "new/duplicate",
    ] {
        assert!(text.contains(expected), "missing {expected:?}");
    }
    key(&mut popup, KeyCode::Char('n'));
    popup.editor = Some(TextArea::new(vec!["Night".into()]));
    key(&mut popup, KeyCode::Enter);
    let text = render(&mut popup, &mut terminal);
    assert!(text.contains("Configuration · Night"));
    key(&mut popup, KeyCode::Esc);
    let text = render(&mut popup, &mut terminal);
    assert!(text.contains("Discard theme edits? y/n"));
}

#[test]
fn narrow_theme_list_keeps_actions_and_discard_choices_visible() {
    for width in [25, 30] {
        let (_, mut popup) = lazy_popup();
        appearance(&mut popup);
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
        let render = |popup: &mut ConfigPopup, terminal: &mut Terminal<TestBackend>| {
            terminal
                .draw(|frame| popup.render_widget(frame, frame.area(), &Styles::default()))
                .unwrap();
            terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>()
        };
        let text = render(&mut popup, &mut terminal);
        for expected in ["a Activate", "e Edit", "n New", "Tab Next", "Esc Close"] {
            assert!(
                text.contains(expected),
                "missing {expected:?} at width {width}"
            );
        }
        key(&mut popup, KeyCode::Char('n'));
        let text = render(&mut popup, &mut terminal);
        for expected in ["Enter Accept", "Esc Cancel", "^S Save"] {
            assert!(
                text.contains(expected),
                "missing {expected:?} at width {width}"
            );
        }
        popup.editor = Some(TextArea::new(vec!["Night".into()]));
        key(&mut popup, KeyCode::Enter);
        key(&mut popup, KeyCode::Esc);
        assert!(render(&mut popup, &mut terminal).contains("Discard? y/n"));
    }
}

#[test]
fn unavailable_theme_directory_does_not_block_other_configuration_tabs() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("themes"), "keep").unwrap();
    let mut popup = ConfigPopup::new(
        ConfigDocument::load(directory.path().join("config.toml")).unwrap(),
        ThemeStore::from_directory(directory.path().join("themes")).unwrap(),
        &Settings::default(),
        &Styles::default(),
        None,
        37,
    )
    .unwrap();
    assert!(popup.error);
    assert!(popup.status.contains("Could not list themes"));
    key(&mut popup, KeyCode::Enter);
    assert!(popup.settings_document.is_dirty());
    appearance(&mut popup);
    assert!(popup.theme_catalog[0].error.is_some());
    assert!(popup.selected_theme().is_err());
    key(&mut popup, KeyCode::Char('e'));
    assert!(popup.themes_document.is_none());
    key(&mut popup, KeyCode::Char('n'));
    assert!(popup.editor.is_none());
    assert_eq!(
        fs::read_to_string(directory.path().join("themes")).unwrap(),
        "keep"
    );
}

#[test]
fn builtin_fallback_is_not_the_custom_tui_theme_until_activated() {
    let (directory, mut popup) = lazy_popup();
    fs::create_dir(directory.path().join("themes")).unwrap();
    fs::write(
        directory.path().join("themes/theme_tui.toml"),
        "[msgbox]\nerror = 'Green'\n",
    )
    .unwrap();
    popup.active_theme_id = None;
    assert!(!popup.theme_is_active());
    appearance(&mut popup);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|frame| popup.render_widget(frame, frame.area(), &Styles::default()))
        .unwrap();
    let text: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(text.contains("Built-in Tui fallback active"));
    key(&mut popup, KeyCode::Char('e'));
    assert_eq!(popup.edited_theme_id(), Some("tui"));
    assert!(!popup.theme_is_active());
    select(&mut popup, Tab::Appearance, &["msgbox", "error"]);
    let field = popup.current_field().clone();
    popup
        .apply_value(&field, Some(Value::String("Yellow".into())))
        .unwrap();
    let styles = Styles::deserialize(&popup.themes_document.as_ref().unwrap().text()).unwrap();
    popup.themes_document_mut().unwrap().save().unwrap();
    popup.mark_appearance_saved(&styles);
    assert!(!popup.theme_is_active());
    assert!(popup.status.contains("Activate it"));
    popup.mark_theme_activated("tui");
    assert!(popup.theme_is_active());
}

#[test]
fn medium_theme_list_keeps_navigation_footer_visible() {
    for width in [55, 60, 70, 75] {
        let (_, mut popup) = lazy_popup();
        appearance(&mut popup);
        let mut terminal = Terminal::new(TestBackend::new(width, 24)).unwrap();
        terminal
            .draw(|frame| popup.render_widget(frame, frame.area(), &Styles::default()))
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        for expected in [
            "Enter/a Activate",
            "e Edit",
            "n New/duplicate",
            "Tab Sections",
            "Esc Close",
        ] {
            assert!(
                text.contains(expected),
                "missing {expected:?} at width {width}"
            );
        }
    }
}

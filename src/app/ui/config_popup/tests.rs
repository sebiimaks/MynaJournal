use std::fs;

use ratatui::{Terminal, backend::TestBackend};

use super::*;

fn popup(settings: &str, themes: &str) -> (tempfile::TempDir, ConfigPopup) {
    let directory = tempfile::tempdir().unwrap();
    let settings_path = directory.path().join("config.toml");
    let themes_path = directory.path().join("themes.toml");
    fs::write(&settings_path, settings).unwrap();
    fs::write(&themes_path, themes).unwrap();
    let popup = ConfigPopup::new(
        ConfigDocument::load(settings_path).unwrap(),
        ConfigDocument::load(themes_path).unwrap(),
        &toml::from_str(settings).unwrap(),
        &Styles::deserialize(themes).unwrap(),
        37,
    )
    .unwrap();
    (directory, popup)
}

fn key(popup: &mut ConfigPopup, code: KeyCode) -> ConfigAction {
    popup.handle_input(&Input::new(code, KeyModifiers::NONE))
}

fn select(popup: &mut ConfigPopup, tab: Tab, path: &[&str]) {
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
    assert_eq!(popup.handle_input(&save), ConfigAction::SaveAppearance);
    key(&mut popup, KeyCode::Tab);
    assert_eq!(popup.handle_input(&save), ConfigAction::SaveLayout(37));
    key(&mut popup, KeyCode::Char('d'));
    assert_eq!(popup.handle_input(&save), ConfigAction::SaveLayout(30));
    assert!(!popup.settings_document.is_dirty());
    assert!(!popup.themes_document.is_dirty());
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
    let actual = Styles::deserialize(&popup.themes_document.text()).unwrap();
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
        Styles::deserialize(&popup.themes_document.text())
            .unwrap()
            .editor
            .cursor_normal
            .fg,
        None
    );
    popup.restore_default();
    assert_eq!(
        Styles::deserialize(&popup.themes_document.text())
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
    assert_eq!(appearance.len(), (7 + 10 + 8) * 4 + 4);
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

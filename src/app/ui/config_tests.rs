use std::fs;

use backend::{DataProvider, EntriesDTO, Entry, EntryDraft, ModifyEntryError};
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend, style::Color};

use crate::{
    app::{TagColors, history::HistoryStack, keymap::Input},
    settings::{ConfigContext, Settings},
};

use super::{
    App, ControlType, Popup, Styles, UIComponents,
    config_popup::{ConfigAction, ConfigPopup},
    editor::EditorMode,
    themes::ThemeStore,
};

struct NoAccessProvider;

impl DataProvider for NoAccessProvider {
    async fn load_all_entries(&mut self) -> anyhow::Result<Vec<Entry>> {
        panic!("configuration must not load journals")
    }

    async fn add_entry(&mut self, _: EntryDraft) -> Result<Entry, ModifyEntryError> {
        panic!("configuration must not add journals")
    }

    async fn restore_entry(&mut self, _: Entry) -> Result<Entry, ModifyEntryError> {
        panic!("configuration must not restore journals")
    }

    async fn remove_entry(&mut self, _: u32) -> anyhow::Result<()> {
        panic!("configuration must not remove journals")
    }

    async fn update_entry(&mut self, _: Entry) -> Result<Entry, ModifyEntryError> {
        panic!("configuration must not update journals")
    }

    async fn get_export_object(&mut self, _: &[u32]) -> anyhow::Result<EntriesDTO> {
        panic!("configuration must not export journals")
    }

    async fn assign_priority_to_entries(&mut self, _: u32) -> anyhow::Result<()> {
        panic!("configuration must not change journal priorities")
    }
}

fn setup(
    config: &str,
) -> (
    tempfile::TempDir,
    App<NoAccessProvider>,
    UIComponents<'static>,
) {
    let dir = tempfile::tempdir().unwrap();
    let settings_path = dir.path().join("config.toml");
    fs::write(&settings_path, config).unwrap();
    let mut settings: Settings = toml::from_str(config).unwrap();
    settings.complete_missing_options().unwrap();
    settings.app_state_dir = Some(dir.path().join("state"));
    let mut app = App::new(NoAccessProvider, settings);
    app.config_context = Some(ConfigContext {
        settings_path,
        themes_path: dir.path().join("themes"),
        active_theme_id: Some("tui".into()),
        theme_warning: None,
        launch_details: vec!["Test launch overrides".into()],
    });
    app.entries.push(Entry::new(
        7,
        chrono::Utc::now(),
        "Journal".into(),
        "Saved journal\n世界 e\u{301} 👩‍👩‍👧‍👦".into(),
        vec!["work".into()],
        None,
    ));
    app.update_colored_tags();
    let mut ui = UIComponents::new(Styles::default());
    ui.set_current_entry(Some(7), &mut app);
    (dir, app, ui)
}

fn key(code: KeyCode) -> Input {
    Input::new(code, KeyModifiers::NONE)
}

fn popup<'a>(ui: &'a mut UIComponents<'_>) -> &'a mut ConfigPopup {
    match ui.popup_stack.last_mut() {
        Some(Popup::Config(popup)) => popup,
        _ => panic!("expected configuration popup"),
    }
}

fn set_setting(ui: &mut UIComponents<'_>, path: &[&str], value: toml::Value) {
    popup(ui)
        .settings_document_mut()
        .set(path, Some(value))
        .unwrap();
}

async fn press(ui: &mut UIComponents<'_>, app: &mut App<NoAccessProvider>, code: KeyCode) {
    ui.handle_input(&key(code), app).await.unwrap();
}

async fn control(ui: &mut UIComponents<'_>, app: &mut App<NoAccessProvider>, character: char) {
    ui.handle_input(
        &Input::new(KeyCode::Char(character), KeyModifiers::CONTROL),
        app,
    )
    .await
    .unwrap();
}

async fn edit_text(ui: &mut UIComponents<'_>, app: &mut App<NoAccessProvider>, text: &str) {
    press(ui, app, KeyCode::Enter).await;
    control(ui, app, 'a').await;
    control(ui, app, 'k').await;
    for character in text.chars() {
        press(ui, app, KeyCode::Char(character)).await;
    }
    press(ui, app, KeyCode::Enter).await;
}

async fn appearance(ui: &mut UIComponents<'_>, app: &mut App<NoAccessProvider>) {
    for _ in 0..4 {
        press(ui, app, KeyCode::Tab).await;
    }
}

fn rendered_tag_colors(
    ui: &mut UIComponents<'_>,
    app: &App<NoAccessProvider>,
    tag: &str,
) -> TagColors {
    // The highlighted row would paint over the chip colors.
    ui.entries_list.state.select(None);
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| ui.render_ui(frame, app)).unwrap();
    let buffer = terminal.backend().buffer();
    for y in 0..buffer.area.height {
        for x in 0..=buffer.area.width - tag.len() as u16 {
            if tag
                .chars()
                .zip(x..)
                .all(|(character, x)| buffer[(x, y)].symbol() == character.to_string())
            {
                let cell = &buffer[(x, y)];
                return TagColors::new(cell.fg, cell.bg);
            }
        }
    }
    panic!("tag {tag:?} is not rendered");
}

#[tokio::test]
async fn config_shortcut_opens_in_browsing_normal_visual_and_multi_select_modes() {
    for modifiers in [KeyModifiers::NONE, KeyModifiers::SHIFT] {
        for mode in 0..4 {
            let (_dir, mut app, mut ui) = setup("");
            match mode {
                1 => ui.change_active_control(ControlType::EntryContentTxt),
                2 => {
                    ui.change_active_control(ControlType::EntryContentTxt);
                    ui.editor.set_editor_mode(EditorMode::Visual);
                }
                3 => {
                    ui.entries_list.multi_select_mode = true;
                    app.selected_entries.insert(7);
                }
                _ => {}
            }
            let original_mode = ui.editor.get_editor_mode();
            let original_focus = ui.active_control;
            let original_selection = app.selected_entries.clone();

            ui.handle_input(&Input::new(KeyCode::Char('!'), modifiers), &mut app)
                .await
                .unwrap();

            assert!(matches!(ui.popup_stack.last(), Some(Popup::Config(_))));
            assert_eq!(ui.active_control, original_focus);
            assert_eq!(ui.editor.get_editor_mode(), original_mode);
            assert_eq!(app.selected_entries, original_selection);
            press(&mut ui, &mut app, KeyCode::Esc).await;
            assert!(!ui.has_popup());
        }
    }
}

#[tokio::test]
async fn config_shortcut_remains_literal_punctuation_in_insert_mode() {
    for modifiers in [KeyModifiers::NONE, KeyModifiers::SHIFT] {
        let (_dir, mut app, mut ui) = setup("");
        ui.change_active_control(ControlType::EntryContentTxt);
        ui.editor.set_editor_mode(EditorMode::Insert);
        let original = ui.editor.get_content();

        ui.handle_input(&Input::new(KeyCode::Char('!'), modifiers), &mut app)
            .await
            .unwrap();

        assert!(!ui.has_popup());
        assert_eq!(ui.editor.get_content(), format!("{original}!"));
        assert!(ui.has_unsaved());
    }
}

#[tokio::test]
async fn opening_and_cancelling_does_not_create_or_rewrite_configuration() {
    let (dir, mut app, mut ui) = setup("# Keep this formatting\neditor_word_wrap=true\n");
    let original = fs::read(dir.path().join("config.toml")).unwrap();
    press(&mut ui, &mut app, KeyCode::Char('!')).await;
    press(&mut ui, &mut app, KeyCode::Esc).await;
    assert_eq!(fs::read(dir.path().join("config.toml")).unwrap(), original);
    assert!(!dir.path().join("themes").exists());
    assert!(!dir.path().join("state").exists());

    fs::remove_file(dir.path().join("config.toml")).unwrap();
    press(&mut ui, &mut app, KeyCode::Char('!')).await;
    press(&mut ui, &mut app, KeyCode::Esc).await;
    assert!(!dir.path().join("config.toml").exists());
    assert!(!dir.path().join("themes").exists());
}

#[tokio::test]
async fn declining_then_confirming_discard_keeps_journal_and_saved_settings_unchanged() {
    let (dir, mut app, mut ui) = setup("colored_tags = true\n");
    ui.editor.set_entry_content("Unsaved 世界", &app);
    press(&mut ui, &mut app, KeyCode::Char('!')).await;
    press(&mut ui, &mut app, KeyCode::Enter).await;
    assert!(popup(&mut ui).settings_document_mut().is_dirty());
    press(&mut ui, &mut app, KeyCode::Esc).await;
    assert!(ui.has_popup());
    press(&mut ui, &mut app, KeyCode::Char('n')).await;
    assert!(ui.has_popup());
    assert!(popup(&mut ui).settings_document_mut().is_dirty());
    press(&mut ui, &mut app, KeyCode::Esc).await;
    press(&mut ui, &mut app, KeyCode::Char('y')).await;

    assert!(!ui.has_popup());
    assert!(app.settings.colored_tags);
    assert_eq!(ui.editor.get_content(), "Unsaved 世界");
    assert!(ui.has_unsaved());
    assert_eq!(
        fs::read_to_string(dir.path().join("config.toml")).unwrap(),
        "colored_tags = true\n"
    );
}

#[tokio::test]
async fn control_s_saves_the_current_configuration_without_saving_the_journal() {
    let (dir, mut app, mut ui) = setup("colored_tags = true\n");
    ui.editor.set_entry_content("Unsaved 世界", &app);
    press(&mut ui, &mut app, KeyCode::Char('!')).await;
    press(&mut ui, &mut app, KeyCode::Enter).await;
    ui.handle_input(
        &Input::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        &mut app,
    )
    .await
    .unwrap();

    assert!(!app.settings.colored_tags);
    assert!(!popup(&mut ui).settings_document_mut().is_dirty());
    assert_eq!(ui.editor.get_content(), "Unsaved 世界");
    assert!(ui.has_unsaved());
    let saved: Settings =
        toml::from_str(&fs::read_to_string(dir.path().join("config.toml")).unwrap()).unwrap();
    assert!(!saved.colored_tags);
}

#[tokio::test]
async fn popup_keeps_shortcuts_and_navigation_out_of_the_journal() {
    let (_dir, mut app, mut ui) = setup("");
    ui.change_active_control(ControlType::EntryContentTxt);
    ui.editor
        .set_entry_content("Unfinished 世界\r\nsecond line", &app);
    let original = ui.editor.get_content();
    let entries = app.entries.clone();
    press(&mut ui, &mut app, KeyCode::Char('!')).await;
    for code in [KeyCode::Char('s'), KeyCode::Delete, KeyCode::Up] {
        press(&mut ui, &mut app, code).await;
    }
    ui.handle_input(&Input::new(KeyCode::Char('.'), KeyModifiers::ALT), &mut app)
        .await
        .unwrap();
    assert_eq!(ui.editor.get_content(), original);
    assert!(ui.has_unsaved());
    assert_eq!(app.entries, entries);
    assert_eq!(app.state.entries_list_percentage(), 30);
    assert!(ui.has_popup());
}

#[tokio::test]
async fn saving_wrap_preserves_dirty_text_cursor_selection_and_editor_undo() {
    let (_dir, mut app, mut ui) = setup("");
    ui.change_active_control(ControlType::EntryContentTxt);
    ui.editor.set_entry_content("Unfinished 世界\r\nabc", &app);
    ui.editor.set_editor_mode(EditorMode::Insert);
    ui.editor
        .handle_input_prioritized(&key(KeyCode::Char('Z')), &app)
        .unwrap();
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| ui.render_ui(frame, &app)).unwrap();
    ui.editor.set_editor_mode(EditorMode::Normal);
    ui.editor.set_editor_mode(EditorMode::Visual);
    ui.editor.handle_input(&key(KeyCode::Left), &app).unwrap();
    assert_eq!(ui.editor.get_editor_mode(), EditorMode::Visual);
    let original = ui.editor.get_content();
    app.selected_entries.insert(7);
    let entries = app.entries.clone();
    app.history
        .register_change_content(HistoryStack::Undo, &app.entries[0]);
    let history = format!("{:?}", app.history);

    press(&mut ui, &mut app, KeyCode::Char('!')).await;
    set_setting(&mut ui, &["editor_word_wrap"], false.into());
    ui.save_config(ConfigAction::SaveSettings, &mut app)
        .unwrap();
    press(&mut ui, &mut app, KeyCode::Esc).await;

    assert!(!app.settings.editor_word_wrap);
    assert_eq!(ui.active_control, ControlType::EntryContentTxt);
    assert_eq!(ui.editor.get_editor_mode(), EditorMode::Visual);
    assert_eq!(ui.editor.get_content(), original);
    assert!(ui.has_unsaved());
    assert_eq!(app.entries, entries);
    assert_eq!(app.current_entry_id, Some(7));
    assert!(app.selected_entries.contains(&7));
    assert_eq!(format!("{:?}", app.history), history);

    ui.editor
        .handle_input(&key(KeyCode::Char('d')), &app)
        .unwrap();
    assert_eq!(ui.editor.get_content(), "Unfinished 世界\r\nabc");
    ui.editor
        .handle_input(&key(KeyCode::Char('u')), &app)
        .unwrap();
    assert_eq!(ui.editor.get_content(), original);
    ui.editor
        .handle_input(&key(KeyCode::Char('u')), &app)
        .unwrap();
    assert_eq!(ui.editor.get_content(), "Unfinished 世界\r\nabc");
}

#[tokio::test]
async fn settings_save_preserves_auto_title_forms_and_unknown_backend_sections() {
    for auto_title in ["'My daily note'", "{ kind = 'date' }"] {
        let input = format!(
            "# User config\nauto_title = {auto_title}\n\n[future_backend]\nvalue = 'keep me'\n\n[json_backend]\nfile_path = 'keep.json'\n\n[sqlite_backend]\nfile_path = 'keep.db'\n\n[vjournal_backend]\ndirectory = 'keep-journals'\n\n[file_backend]\ndirectory = 'keep-notes'\nformat = 'markdown'\n"
        );
        let (dir, mut app, mut ui) = setup(&input);
        let title = app.settings.auto_title.clone();
        press(&mut ui, &mut app, KeyCode::Char('!')).await;
        set_setting(&mut ui, &["editor_word_wrap"], false.into());
        ui.save_config(ConfigAction::SaveSettings, &mut app)
            .unwrap();

        let saved = fs::read_to_string(dir.path().join("config.toml")).unwrap();
        let reloaded: Settings = toml::from_str(&saved).unwrap();
        let values: toml::Value = toml::from_str(&saved).unwrap();
        assert_eq!(reloaded.auto_title, title);
        assert_eq!(app.settings.auto_title, title);
        assert_eq!(values["future_backend"]["value"].as_str(), Some("keep me"));
        assert_eq!(
            values["json_backend"]["file_path"].as_str(),
            Some("keep.json")
        );
        assert_eq!(
            values["sqlite_backend"]["file_path"].as_str(),
            Some("keep.db")
        );
        assert_eq!(
            values["vjournal_backend"]["directory"].as_str(),
            Some("keep-journals")
        );
        assert_eq!(
            values["file_backend"]["directory"].as_str(),
            Some("keep-notes")
        );
        assert_eq!(values["file_backend"]["format"].as_str(), Some("markdown"));
        assert!(saved.contains("# User config"));
    }
}

#[tokio::test]
async fn restart_settings_save_without_changing_active_storage_or_history() {
    let (dir, mut app, mut ui) = setup("history_limit = 12\n");
    let original = app.settings.clone();
    app.history
        .register_change_content(HistoryStack::Undo, &app.entries[0]);
    let history = format!("{:?}", app.history);
    press(&mut ui, &mut app, KeyCode::Char('!')).await;
    set_setting(&mut ui, &["history_limit"], 3.into());
    set_setting(
        &mut ui,
        &["backend_type"],
        format!("{:?}", crate::settings::BackendType::default()).into(),
    );
    set_setting(&mut ui, &["app_state_dir"], "/new/state/directory".into());
    #[cfg(feature = "json")]
    set_setting(
        &mut ui,
        &["json_backend", "file_path"],
        "/new/journals.json".into(),
    );
    #[cfg(feature = "sqlite")]
    set_setting(
        &mut ui,
        &["sqlite_backend", "file_path"],
        "/new/journals.db".into(),
    );
    #[cfg(feature = "vjournal")]
    set_setting(
        &mut ui,
        &["vjournal_backend", "directory"],
        "/new/vjournal".into(),
    );
    #[cfg(feature = "file")]
    {
        set_setting(&mut ui, &["file_backend", "directory"], "/new/notes".into());
        set_setting(&mut ui, &["file_backend", "format"], "markdown".into());
    }
    ui.save_config(ConfigAction::SaveSettings, &mut app)
        .unwrap();

    assert_eq!(app.settings.history_limit, original.history_limit);
    assert_eq!(app.settings.app_state_dir, original.app_state_dir);
    assert_eq!(app.settings.backend_type, original.backend_type);
    assert_eq!(format!("{:?}", app.history), history);
    #[cfg(feature = "json")]
    assert_eq!(
        app.settings.json_backend.file_path,
        original.json_backend.file_path
    );
    #[cfg(feature = "sqlite")]
    assert_eq!(
        app.settings.sqlite_backend.file_path,
        original.sqlite_backend.file_path
    );
    #[cfg(feature = "vjournal")]
    assert_eq!(
        app.settings.vjournal_backend.directory,
        original.vjournal_backend.directory
    );
    #[cfg(feature = "file")]
    {
        assert_eq!(
            app.settings.file_backend.directory,
            original.file_backend.directory
        );
        assert_eq!(
            app.settings.file_backend.format,
            original.file_backend.format
        );
    }
    let saved: Settings =
        toml::from_str(&fs::read_to_string(dir.path().join("config.toml")).unwrap()).unwrap();
    assert_eq!(saved.history_limit, 3);
    assert_eq!(
        saved.app_state_dir.unwrap().to_str(),
        Some("/new/state/directory")
    );
    assert_eq!(
        saved.backend_type,
        Some(crate::settings::BackendType::default())
    );
    #[cfg(feature = "json")]
    assert_eq!(
        saved.json_backend.file_path.unwrap().to_str(),
        Some("/new/journals.json")
    );
    #[cfg(feature = "sqlite")]
    assert_eq!(
        saved.sqlite_backend.file_path.unwrap().to_str(),
        Some("/new/journals.db")
    );
    #[cfg(feature = "vjournal")]
    assert_eq!(
        saved.vjournal_backend.directory.unwrap().to_str(),
        Some("/new/vjournal")
    );
    #[cfg(feature = "file")]
    {
        assert_eq!(
            saved.file_backend.directory.unwrap().to_str(),
            Some("/new/notes")
        );
        assert_eq!(saved.file_backend.format, backend::FileFormat::Markdown);
    }
}

#[tokio::test]
async fn unrelated_save_preserves_stored_backend_values_and_active_cli_overrides() {
    let config = format!(
        "backend_type = '{:?}'\neditor_word_wrap = true\n\n[json_backend]\nfile_path = 'saved.json'\n\n[sqlite_backend]\nfile_path = 'saved.db'\n\n[vjournal_backend]\ndirectory = 'saved-vjournal'\n\n[file_backend]\ndirectory = 'saved-notes'\nformat = 'markdown'\n",
        crate::settings::BackendType::default()
    );
    let (dir, mut app, mut ui) = setup(&config);
    let stored: toml::Value = toml::from_str(&config).unwrap();
    #[cfg(feature = "json")]
    {
        app.settings.backend_type = Some(crate::settings::BackendType::Json);
        app.settings.json_backend.file_path = Some(dir.path().join("cli.json"));
    }
    #[cfg(feature = "sqlite")]
    {
        app.settings.sqlite_backend.file_path = Some(dir.path().join("cli.db"));
    }
    #[cfg(feature = "vjournal")]
    {
        app.settings.vjournal_backend.directory = Some(dir.path().join("cli-vjournal"));
    }
    #[cfg(feature = "file")]
    {
        app.settings.file_backend.directory = Some(dir.path().join("cli-notes"));
    }
    let active = app.settings.clone();
    press(&mut ui, &mut app, KeyCode::Char('!')).await;
    set_setting(&mut ui, &["editor_word_wrap"], false.into());
    ui.save_config(ConfigAction::SaveSettings, &mut app)
        .unwrap();

    let saved: toml::Value =
        toml::from_str(&fs::read_to_string(dir.path().join("config.toml")).unwrap()).unwrap();
    for name in [
        "backend_type",
        "json_backend",
        "sqlite_backend",
        "vjournal_backend",
        "file_backend",
    ] {
        assert_eq!(saved[name], stored[name]);
    }
    assert!(!app.settings.editor_word_wrap);
    assert_eq!(app.settings.backend_type, active.backend_type);
    #[cfg(feature = "json")]
    assert_eq!(
        app.settings.json_backend.file_path,
        active.json_backend.file_path
    );
    #[cfg(feature = "sqlite")]
    assert_eq!(
        app.settings.sqlite_backend.file_path,
        active.sqlite_backend.file_path
    );
    #[cfg(feature = "vjournal")]
    assert_eq!(
        app.settings.vjournal_backend.directory,
        active.vjournal_backend.directory
    );
    #[cfg(feature = "file")]
    {
        assert_eq!(
            app.settings.file_backend.directory,
            active.file_backend.directory
        );
        assert_eq!(app.settings.file_backend.format, active.file_backend.format);
    }
    assert!(!dir.path().join("cli.json").exists());
    assert!(!dir.path().join("cli.db").exists());
    assert!(!dir.path().join("cli-vjournal").exists());
    assert!(!dir.path().join("cli-notes").exists());
}

#[tokio::test]
async fn saving_colored_tags_rebuilds_the_existing_journal_tag_cache() {
    let (_dir, mut app, mut ui) = setup("colored_tags = false\n");
    assert!(app.get_color_for_tag("work").is_none());
    press(&mut ui, &mut app, KeyCode::Char('!')).await;
    set_setting(&mut ui, &["colored_tags"], true.into());
    ui.save_config(ConfigAction::SaveSettings, &mut app)
        .unwrap();
    assert!(app.get_color_for_tag("work").is_some());
    set_setting(&mut ui, &["colored_tags"], false.into());
    ui.save_config(ConfigAction::SaveSettings, &mut app)
        .unwrap();
    assert!(app.get_color_for_tag("work").is_none());
}

#[tokio::test]
async fn activating_theme_recolors_rendered_tag_chips_without_restart() {
    let (dir, mut app, mut ui) = setup("colored_tags = true\n");
    let themes = dir.path().join("themes");
    fs::create_dir(&themes).unwrap();
    fs::write(
        themes.join("theme_night.toml"),
        "[tags]\npalette = [{ fg = 'White', bg = 'Blue' }]\n",
    )
    .unwrap();
    let builtin = TagColors::new(Color::Black, Color::LightCyan);
    assert_eq!(rendered_tag_colors(&mut ui, &app, "work"), builtin);

    press(&mut ui, &mut app, KeyCode::Char('!')).await;
    appearance(&mut ui, &mut app).await;
    press(&mut ui, &mut app, KeyCode::Down).await;
    press(&mut ui, &mut app, KeyCode::Enter).await;
    press(&mut ui, &mut app, KeyCode::Esc).await;

    assert!(!ui.has_popup());
    assert_eq!(ui.active_theme_id.as_deref(), Some("night"));
    assert_eq!(
        rendered_tag_colors(&mut ui, &app, "work"),
        TagColors::new(Color::White, Color::Blue)
    );

    press(&mut ui, &mut app, KeyCode::Char('!')).await;
    appearance(&mut ui, &mut app).await;
    press(&mut ui, &mut app, KeyCode::Up).await;
    press(&mut ui, &mut app, KeyCode::Enter).await;
    press(&mut ui, &mut app, KeyCode::Esc).await;

    assert_eq!(ui.active_theme_id.as_deref(), Some("tui"));
    assert_eq!(rendered_tag_colors(&mut ui, &app, "work"), builtin);
}

#[tokio::test]
async fn saving_tag_palette_recolors_tags_only_for_the_active_theme() {
    let (dir, mut app, mut ui) = setup("colored_tags = true\n");
    let themes = dir.path().join("themes");
    fs::create_dir(&themes).unwrap();
    fs::write(themes.join("theme_night.toml"), "").unwrap();
    let builtin = Some(TagColors::new(Color::Black, Color::LightCyan));
    let white_on_blue = Some(TagColors::new(Color::White, Color::Blue));

    press(&mut ui, &mut app, KeyCode::Char('!')).await;
    appearance(&mut ui, &mut app).await;
    press(&mut ui, &mut app, KeyCode::Down).await;
    press(&mut ui, &mut app, KeyCode::Char('e')).await;
    press(&mut ui, &mut app, KeyCode::End).await;
    edit_text(&mut ui, &mut app, "White on Blue").await;
    control(&mut ui, &mut app, 's').await;
    assert_eq!(app.get_color_for_tag("work"), builtin);

    press(&mut ui, &mut app, KeyCode::Esc).await;
    press(&mut ui, &mut app, KeyCode::Enter).await;
    assert_eq!(ui.active_theme_id.as_deref(), Some("night"));
    assert_eq!(app.get_color_for_tag("work"), white_on_blue);

    press(&mut ui, &mut app, KeyCode::Char('e')).await;
    press(&mut ui, &mut app, KeyCode::End).await;
    edit_text(&mut ui, &mut app, "Black on Yellow").await;
    assert_eq!(app.get_color_for_tag("work"), white_on_blue);
    control(&mut ui, &mut app, 's').await;
    assert_eq!(
        app.get_color_for_tag("work"),
        Some(TagColors::new(Color::Black, Color::Yellow))
    );
    let saved =
        Styles::deserialize(&fs::read_to_string(themes.join("theme_night.toml")).unwrap()).unwrap();
    assert_eq!(
        saved.tags.palette,
        [TagColors::new(Color::Black, Color::Yellow)]
    );
}

#[tokio::test]
async fn enabling_colored_tags_uses_the_active_theme_palette() {
    let (dir, mut app, mut ui) = setup("colored_tags = false\n");
    let themes = dir.path().join("themes");
    fs::create_dir(&themes).unwrap();
    fs::write(
        themes.join("theme_night.toml"),
        "[tags]\npalette = [{ fg = 'White', bg = 'Blue' }]\n",
    )
    .unwrap();

    press(&mut ui, &mut app, KeyCode::Char('!')).await;
    appearance(&mut ui, &mut app).await;
    press(&mut ui, &mut app, KeyCode::Down).await;
    press(&mut ui, &mut app, KeyCode::Enter).await;
    assert_eq!(ui.active_theme_id.as_deref(), Some("night"));
    assert!(app.get_color_for_tag("work").is_none());

    set_setting(&mut ui, &["colored_tags"], true.into());
    ui.save_config(ConfigAction::SaveSettings, &mut app)
        .unwrap();
    assert_eq!(
        app.get_color_for_tag("work"),
        Some(TagColors::new(Color::White, Color::Blue))
    );
}

#[tokio::test]
async fn external_config_change_keeps_the_unsaved_draft_and_runtime_settings() {
    let (dir, mut app, mut ui) = setup("editor_word_wrap = true\n");
    ui.change_active_control(ControlType::EntryContentTxt);
    ui.editor.set_entry_content("Unsaved journal 世界", &app);
    press(&mut ui, &mut app, KeyCode::Char('!')).await;
    set_setting(&mut ui, &["editor_word_wrap"], false.into());
    let external = "editor_word_wrap = true\n# Changed outside the app\n";
    fs::write(dir.path().join("config.toml"), external).unwrap();

    assert!(
        ui.save_config(ConfigAction::SaveSettings, &mut app)
            .is_err()
    );
    ui.handle_input(
        &Input::new(KeyCode::Char('s'), KeyModifiers::CONTROL),
        &mut app,
    )
    .await
    .unwrap();
    assert!(app.settings.editor_word_wrap);
    assert!(popup(&mut ui).settings_document_mut().is_dirty());
    assert_eq!(
        popup(&mut ui)
            .settings_document_mut()
            .value(&["editor_word_wrap"]),
        Some(false.into())
    );
    assert_eq!(
        fs::read_to_string(dir.path().join("config.toml")).unwrap(),
        external
    );
    assert_eq!(ui.editor.get_content(), "Unsaved journal 世界");
    assert!(ui.has_unsaved());
}

#[tokio::test]
async fn failed_settings_write_does_not_apply_the_draft() {
    let (dir, mut app, mut ui) = setup("");
    fs::remove_file(dir.path().join("config.toml")).unwrap();
    let blocker = dir.path().join("not-a-directory");
    app.config_context.as_mut().unwrap().settings_path = blocker.join("config.toml");
    ui.show_config(&app).unwrap();
    set_setting(&mut ui, &["editor_word_wrap"], false.into());
    fs::write(&blocker, "keep").unwrap();

    assert!(
        ui.save_config(ConfigAction::SaveSettings, &mut app)
            .is_err()
    );
    assert!(app.settings.editor_word_wrap);
    assert!(popup(&mut ui).settings_document_mut().is_dirty());
    assert_eq!(fs::read_to_string(blocker).unwrap(), "keep");
}

#[tokio::test]
async fn appearance_and_settings_have_independent_save_states() {
    let (dir, mut app, mut ui) = setup("");
    press(&mut ui, &mut app, KeyCode::Char('!')).await;
    set_setting(&mut ui, &["editor_word_wrap"], false.into());
    for _ in 0..4 {
        press(&mut ui, &mut app, KeyCode::Tab).await;
    }
    press(&mut ui, &mut app, KeyCode::Char('e')).await;
    popup(&mut ui)
        .themes_document_mut()
        .unwrap()
        .set(
            &["general", "input_block_active", "fg"],
            Some("Magenta".into()),
        )
        .unwrap();
    ui.save_config(ConfigAction::SaveAppearance, &mut app)
        .unwrap();

    assert_eq!(
        ui.styles.general.input_block_active.fg,
        Some(Color::Magenta)
    );
    assert!(app.settings.editor_word_wrap);
    assert!(popup(&mut ui).settings_document_mut().is_dirty());
    assert!(!popup(&mut ui).themes_document_mut().unwrap().is_dirty());
    assert_eq!(
        fs::read_to_string(dir.path().join("config.toml")).unwrap(),
        ""
    );
    assert!(dir.path().join("themes/theme_tui.toml").exists());

    ui.save_config(ConfigAction::SaveSettings, &mut app)
        .unwrap();
    assert!(!app.settings.editor_word_wrap);
    assert!(!popup(&mut ui).settings_document_mut().is_dirty());
}

#[tokio::test]
async fn divider_uses_existing_app_state_without_an_extra_config_setting() {
    let (dir, mut app, mut ui) = setup("");
    press(&mut ui, &mut app, KeyCode::Char('!')).await;
    ui.save_config(ConfigAction::SaveLayout(45), &mut app)
        .unwrap();

    assert_eq!(app.state.entries_list_percentage(), 45);
    assert_eq!(
        fs::read_to_string(dir.path().join("config.toml")).unwrap(),
        ""
    );
    let state: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(dir.path().join("state/state.json")).unwrap())
            .unwrap();
    assert_eq!(state["entries_list_percentage"], 45);
}

#[tokio::test]
async fn failed_divider_persistence_keeps_the_active_divider_position() {
    let (dir, mut app, mut ui) = setup("");
    let blocker = dir.path().join("blocked-state-directory");
    fs::write(&blocker, "keep").unwrap();
    app.settings.app_state_dir = Some(blocker.clone());
    press(&mut ui, &mut app, KeyCode::Char('!')).await;

    assert!(
        ui.save_config(ConfigAction::SaveLayout(45), &mut app)
            .is_err()
    );
    assert_eq!(app.state.entries_list_percentage(), 30);
    assert_eq!(fs::read_to_string(blocker).unwrap(), "keep");
    assert_eq!(
        fs::read_to_string(dir.path().join("config.toml")).unwrap(),
        ""
    );
}

#[tokio::test]
async fn configuration_renders_all_tabs_in_tiny_terminals() {
    let (_dir, mut app, mut ui) = setup("");
    press(&mut ui, &mut app, KeyCode::Char('!')).await;
    for _ in 0..7 {
        for (width, height) in [(0, 0), (1, 1), (12, 4), (20, 6), (40, 12), (100, 30)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| ui.render_ui(frame, &app)).unwrap();
        }
        press(&mut ui, &mut app, KeyCode::Tab).await;
    }
}

#[tokio::test]
async fn activating_theme_keeps_settings_layout_and_journal_drafts_independent() {
    let config = "# Preserve preferences\ncolored_tags = true\nauto_title = { kind = 'date' }\n\n[json_backend]\nfile_path = 'keep.json'\n\n[sqlite_backend]\nfile_path = 'keep.db'\n\n[vjournal_backend]\ndirectory = 'keep-events'\n\n[file_backend]\ndirectory = 'keep-files'\nformat = 'markdown'\n";
    let (dir, mut app, mut ui) = setup(config);
    let themes = dir.path().join("themes");
    fs::create_dir(&themes).unwrap();
    let theme = "[general.input_block_active]\nfg = 'Magenta'\n";
    fs::write(themes.join("theme_night.toml"), theme).unwrap();
    let original_title = app.settings.auto_title.clone();
    ui.change_active_control(ControlType::EntryContentTxt);
    ui.editor.set_entry_content("Unsaved journal 世界", &app);
    ui.editor.set_editor_mode(EditorMode::Insert);
    ui.editor
        .handle_input_prioritized(&key(KeyCode::Char('Z')), &app)
        .unwrap();
    ui.editor.set_editor_mode(EditorMode::Normal);
    let journal_draft = ui.editor.get_content();
    let entries = app.entries.clone();
    app.history
        .register_change_content(HistoryStack::Undo, &app.entries[0]);
    let history = format!("{:?}", app.history);

    press(&mut ui, &mut app, KeyCode::Char('!')).await;
    press(&mut ui, &mut app, KeyCode::Enter).await;
    appearance(&mut ui, &mut app).await;
    press(&mut ui, &mut app, KeyCode::Tab).await;
    edit_text(&mut ui, &mut app, "45").await;
    press(&mut ui, &mut app, KeyCode::BackTab).await;
    press(&mut ui, &mut app, KeyCode::Down).await;
    press(&mut ui, &mut app, KeyCode::Enter).await;

    assert_eq!(ui.active_theme_id.as_deref(), Some("night"));
    assert_eq!(app.settings.theme.as_deref(), Some("night"));
    assert_eq!(
        ui.styles.general.input_block_active.fg,
        Some(Color::Magenta)
    );
    assert!(app.settings.colored_tags);
    assert_eq!(app.settings.auto_title, original_title);
    assert_eq!(app.state.entries_list_percentage(), 30);
    assert!(!dir.path().join("state").exists());
    assert_eq!(ui.editor.get_content(), journal_draft);
    assert!(ui.has_unsaved());
    assert_eq!(app.entries, entries);
    assert_eq!(format!("{:?}", app.history), history);
    let draft = popup(&mut ui).settings_document_mut();
    assert!(draft.is_dirty());
    assert_eq!(draft.value(&["colored_tags"]), Some(false.into()));
    let saved_text = fs::read_to_string(dir.path().join("config.toml")).unwrap();
    let saved: toml::Value = toml::from_str(&saved_text).unwrap();
    let original: toml::Value = toml::from_str(config).unwrap();
    assert_eq!(saved["theme"].as_str(), Some("night"));
    for name in [
        "colored_tags",
        "auto_title",
        "json_backend",
        "sqlite_backend",
        "vjournal_backend",
        "file_backend",
    ] {
        assert_eq!(saved[name], original[name]);
    }
    assert!(saved_text.contains("# Preserve preferences"));
    assert_eq!(
        fs::read_to_string(themes.join("theme_night.toml")).unwrap(),
        theme
    );

    press(&mut ui, &mut app, KeyCode::Tab).await;
    control(&mut ui, &mut app, 's').await;
    assert_eq!(app.state.entries_list_percentage(), 45);
    assert!(popup(&mut ui).settings_document_mut().is_dirty());
    press(&mut ui, &mut app, KeyCode::Esc).await;
    press(&mut ui, &mut app, KeyCode::Char('y')).await;
    assert!(!ui.has_popup());
    assert_eq!(ui.editor.get_content(), journal_draft);
    ui.editor
        .handle_input(&key(KeyCode::Char('u')), &app)
        .unwrap();
    assert_eq!(ui.editor.get_content(), "Unsaved journal 世界");
    assert_eq!(format!("{:?}", app.history), history);
}

#[tokio::test]
async fn saving_named_copy_does_not_activate_it_and_active_edits_apply_after_save() {
    let (dir, mut app, mut ui) = setup("");
    let original_styles = toml::Value::try_from(&ui.styles).unwrap();
    press(&mut ui, &mut app, KeyCode::Char('!')).await;
    appearance(&mut ui, &mut app).await;
    press(&mut ui, &mut app, KeyCode::Char('n')).await;
    for character in "Night Blue".chars() {
        press(&mut ui, &mut app, KeyCode::Char(character)).await;
    }
    press(&mut ui, &mut app, KeyCode::Enter).await;
    assert_eq!(popup(&mut ui).edited_theme_id(), Some("night_blue"));
    assert!(!dir.path().join("themes").exists());
    edit_text(&mut ui, &mut app, "Magenta").await;
    control(&mut ui, &mut app, 's').await;

    assert_eq!(ui.active_theme_id.as_deref(), Some("tui"));
    assert_eq!(app.settings.theme, None);
    assert_eq!(toml::Value::try_from(&ui.styles).unwrap(), original_styles);
    assert_eq!(
        fs::read_to_string(dir.path().join("config.toml")).unwrap(),
        ""
    );
    let theme_path = dir.path().join("themes/theme_night_blue.toml");
    let saved: toml::Value = toml::from_str(&fs::read_to_string(&theme_path).unwrap()).unwrap();
    assert_eq!(saved["theme"]["name"].as_str(), Some("Night Blue"));
    assert_eq!(
        saved["general"]["input_block_active"]["fg"].as_str(),
        Some("Magenta")
    );
    assert!(!dir.path().join("themes/theme_tui.toml").exists());

    press(&mut ui, &mut app, KeyCode::Esc).await;
    press(&mut ui, &mut app, KeyCode::Char('a')).await;
    assert_eq!(ui.active_theme_id.as_deref(), Some("night_blue"));
    assert_eq!(
        ui.styles.general.input_block_active.fg,
        Some(Color::Magenta)
    );
    press(&mut ui, &mut app, KeyCode::Char('e')).await;
    edit_text(&mut ui, &mut app, "Blue").await;
    assert_eq!(
        ui.styles.general.input_block_active.fg,
        Some(Color::Magenta)
    );
    control(&mut ui, &mut app, 's').await;
    assert_eq!(ui.styles.general.input_block_active.fg, Some(Color::Blue));
    assert_eq!(app.settings.theme.as_deref(), Some("night_blue"));
    assert!(!popup(&mut ui).themes_document_mut().unwrap().is_dirty());
}

#[tokio::test]
async fn opening_editing_and_cancelling_themes_never_creates_themes_directory() {
    let (dir, mut app, mut ui) = setup("# untouched\n");
    press(&mut ui, &mut app, KeyCode::Char('!')).await;
    appearance(&mut ui, &mut app).await;
    press(&mut ui, &mut app, KeyCode::Char('e')).await;
    edit_text(&mut ui, &mut app, "Magenta").await;
    press(&mut ui, &mut app, KeyCode::Esc).await;
    press(&mut ui, &mut app, KeyCode::Char('n')).await;
    assert!(popup(&mut ui).themes_document_mut().unwrap().is_dirty());
    press(&mut ui, &mut app, KeyCode::Esc).await;
    press(&mut ui, &mut app, KeyCode::Char('y')).await;
    assert!(popup(&mut ui).edited_theme_id().is_none());
    press(&mut ui, &mut app, KeyCode::Char('n')).await;
    for character in "Cancelled Copy".chars() {
        press(&mut ui, &mut app, KeyCode::Char(character)).await;
    }
    press(&mut ui, &mut app, KeyCode::Enter).await;
    assert_eq!(popup(&mut ui).edited_theme_id(), Some("cancelled_copy"));
    press(&mut ui, &mut app, KeyCode::Esc).await;
    press(&mut ui, &mut app, KeyCode::Char('y')).await;
    press(&mut ui, &mut app, KeyCode::Esc).await;

    assert!(!ui.has_popup());
    assert!(!dir.path().join("themes").exists());
    assert_eq!(
        fs::read_to_string(dir.path().join("config.toml")).unwrap(),
        "# untouched\n"
    );
    assert_eq!(ui.active_theme_id.as_deref(), Some("tui"));
}

#[tokio::test]
async fn failed_theme_activation_preserves_runtime_journal_and_settings_draft() {
    for conflict in [false, true] {
        let (dir, mut app, mut ui) = setup("colored_tags = true\n");
        let themes = dir.path().join("themes");
        fs::create_dir(&themes).unwrap();
        let theme_path = themes.join("theme_night.toml");
        fs::write(
            &theme_path,
            "[general.input_block_active]\nfg = 'Magenta'\n",
        )
        .unwrap();
        ui.editor.set_entry_content("Unsaved journal 世界", &app);
        let original_styles = toml::Value::try_from(&ui.styles).unwrap();
        press(&mut ui, &mut app, KeyCode::Char('!')).await;
        press(&mut ui, &mut app, KeyCode::Enter).await;
        appearance(&mut ui, &mut app).await;
        press(&mut ui, &mut app, KeyCode::Down).await;
        let external = if conflict {
            "colored_tags = true\n# Changed outside the app\n"
        } else {
            fs::write(
                &theme_path,
                "[general.input_block_active]\nfg = 'not-a-color'\n",
            )
            .unwrap();
            "colored_tags = true\n"
        };
        fs::write(dir.path().join("config.toml"), external).unwrap();

        press(&mut ui, &mut app, KeyCode::Enter).await;

        assert_eq!(ui.active_theme_id.as_deref(), Some("tui"));
        assert_eq!(app.settings.theme, None);
        assert!(app.settings.colored_tags);
        assert_eq!(toml::Value::try_from(&ui.styles).unwrap(), original_styles);
        assert_eq!(ui.editor.get_content(), "Unsaved journal 世界");
        assert!(ui.has_unsaved());
        let draft = popup(&mut ui).settings_document_mut();
        assert!(draft.is_dirty());
        assert_eq!(draft.value(&["colored_tags"]), Some(false.into()));
        assert_eq!(draft.value(&["theme"]), None);
        assert_eq!(
            fs::read_to_string(dir.path().join("config.toml")).unwrap(),
            external
        );
    }
}

#[tokio::test]
async fn invalid_configured_theme_does_not_prevent_opening_or_saving_other_settings() {
    let (dir, mut app, mut ui) = setup("theme = 'broken'\ncolored_tags = true\n");
    let themes = dir.path().join("themes");
    fs::create_dir(&themes).unwrap();
    let invalid = "[general.input_block_active]\nfg = 'not-a-color'\n";
    fs::write(themes.join("theme_broken.toml"), invalid).unwrap();
    let loaded = ThemeStore::from_directory(themes.clone())
        .unwrap()
        .resolve(app.settings.theme.as_deref());
    assert!(loaded.warning.is_some());
    ui.styles = loaded.styles;
    ui.set_active_theme(loaded.id);

    press(&mut ui, &mut app, KeyCode::Char('!')).await;
    assert!(matches!(ui.popup_stack.last(), Some(Popup::Config(_))));
    press(&mut ui, &mut app, KeyCode::Enter).await;
    control(&mut ui, &mut app, 's').await;
    appearance(&mut ui, &mut app).await;
    press(&mut ui, &mut app, KeyCode::Down).await;
    assert!(popup(&mut ui).selected_theme().is_err());
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| ui.render_ui(frame, &app)).unwrap();

    assert!(!app.settings.colored_tags);
    assert_eq!(ui.active_theme_id, None);
    assert_eq!(app.settings.theme.as_deref(), Some("broken"));
    assert_eq!(
        fs::read_to_string(themes.join("theme_broken.toml")).unwrap(),
        invalid
    );
    let saved: Settings =
        toml::from_str(&fs::read_to_string(dir.path().join("config.toml")).unwrap()).unwrap();
    assert_eq!(saved.theme.as_deref(), Some("broken"));
    assert!(!saved.colored_tags);
}

#[tokio::test]
async fn failed_active_theme_save_keeps_runtime_style_and_theme_draft() {
    let (dir, mut app, mut ui) = setup("");
    let themes = dir.path().join("themes");
    fs::create_dir(&themes).unwrap();
    let theme_path = themes.join("theme_tui.toml");
    fs::write(&theme_path, "# Before editing\n").unwrap();
    let original_styles = toml::Value::try_from(&ui.styles).unwrap();
    press(&mut ui, &mut app, KeyCode::Char('!')).await;
    appearance(&mut ui, &mut app).await;
    press(&mut ui, &mut app, KeyCode::Char('e')).await;
    edit_text(&mut ui, &mut app, "Magenta").await;
    let external = "# Changed externally\n[general.input_block_active]\nfg = 'Blue'\n";
    fs::write(&theme_path, external).unwrap();

    control(&mut ui, &mut app, 's').await;

    assert_eq!(toml::Value::try_from(&ui.styles).unwrap(), original_styles);
    assert_eq!(fs::read_to_string(theme_path).unwrap(), external);
    let draft = popup(&mut ui).themes_document_mut().unwrap();
    assert!(draft.is_dirty());
    let styles = Styles::deserialize(&draft.text()).unwrap();
    assert_eq!(styles.general.input_block_active.fg, Some(Color::Magenta));
}

#[tokio::test]
async fn unavailable_themes_directory_does_not_block_general_configuration() {
    let (dir, mut app, mut ui) = setup("colored_tags = true\n");
    let themes = dir.path().join("themes");
    fs::write(&themes, "A file occupies the themes directory path").unwrap();
    let loaded = ThemeStore::from_directory(themes.clone())
        .unwrap()
        .resolve(None);
    assert!(loaded.warning.is_some());
    ui.styles = loaded.styles;
    ui.set_active_theme(loaded.id);

    press(&mut ui, &mut app, KeyCode::Char('!')).await;
    assert!(matches!(ui.popup_stack.last(), Some(Popup::Config(_))));
    press(&mut ui, &mut app, KeyCode::Enter).await;
    control(&mut ui, &mut app, 's').await;
    assert!(!app.settings.colored_tags);
    appearance(&mut ui, &mut app).await;
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| ui.render_ui(frame, &app)).unwrap();
    press(&mut ui, &mut app, KeyCode::Enter).await;

    assert_eq!(ui.active_theme_id, None);
    assert_eq!(app.settings.theme, None);
    assert!(matches!(ui.popup_stack.last(), Some(Popup::Config(_))));
    assert_eq!(
        fs::read_to_string(themes).unwrap(),
        "A file occupies the themes directory path"
    );
    let saved: Settings =
        toml::from_str(&fs::read_to_string(dir.path().join("config.toml")).unwrap()).unwrap();
    assert!(!saved.colored_tags);
    assert_eq!(saved.theme, None);
    press(&mut ui, &mut app, KeyCode::Esc).await;
    assert!(!ui.has_popup());
}

#[tokio::test]
async fn fallback_defaults_do_not_activate_custom_tui_until_explicit_selection() {
    let (dir, mut app, mut ui) = setup("theme = 'missing'\n");
    let themes = dir.path().join("themes");
    fs::create_dir(&themes).unwrap();
    fs::write(
        themes.join("theme_tui.toml"),
        "[general.input_block_active]\nfg = 'Blue'\n",
    )
    .unwrap();
    let store = ThemeStore::from_directory(themes.clone()).unwrap();
    let loaded = store.resolve(app.settings.theme.as_deref());
    assert!(loaded.warning.is_some());
    assert_eq!(loaded.id, None);
    ui.styles = loaded.styles;
    ui.set_active_theme(loaded.id);
    let fallback_styles = toml::Value::try_from(&ui.styles).unwrap();

    press(&mut ui, &mut app, KeyCode::Char('!')).await;
    appearance(&mut ui, &mut app).await;
    press(&mut ui, &mut app, KeyCode::Char('e')).await;
    assert!(!popup(&mut ui).theme_is_active());
    edit_text(&mut ui, &mut app, "Magenta").await;
    control(&mut ui, &mut app, 's').await;

    assert_eq!(ui.active_theme_id, None);
    assert_eq!(toml::Value::try_from(&ui.styles).unwrap(), fallback_styles);
    assert_eq!(app.settings.theme.as_deref(), Some("missing"));
    assert_eq!(
        fs::read_to_string(dir.path().join("config.toml")).unwrap(),
        "theme = 'missing'\n"
    );
    let saved_styles =
        Styles::deserialize(&fs::read_to_string(themes.join("theme_tui.toml")).unwrap()).unwrap();
    assert_eq!(
        saved_styles.general.input_block_active.fg,
        Some(Color::Magenta)
    );

    press(&mut ui, &mut app, KeyCode::Esc).await;
    press(&mut ui, &mut app, KeyCode::Enter).await;

    assert_eq!(ui.active_theme_id.as_deref(), Some("tui"));
    assert_eq!(app.settings.theme.as_deref(), Some("tui"));
    assert_eq!(
        ui.styles.general.input_block_active.fg,
        Some(Color::Magenta)
    );
    let saved: Settings =
        toml::from_str(&fs::read_to_string(dir.path().join("config.toml")).unwrap()).unwrap();
    let restarted = store.resolve(saved.theme.as_deref());
    assert_eq!(restarted.id.as_deref(), Some("tui"));
    assert!(restarted.warning.is_none());
    assert_eq!(
        toml::Value::try_from(restarted.styles).unwrap(),
        toml::Value::try_from(&ui.styles).unwrap()
    );
}

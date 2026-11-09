use backend::DataProvider;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer};

use super::*;
use crate::app::test::create_default_app;

fn key(code: KeyCode) -> Input {
    Input::new(code, KeyModifiers::NONE)
}

fn control(code: char) -> Input {
    Input::new(KeyCode::Char(code), KeyModifiers::CONTROL)
}

async fn protected_app() -> (App<impl DataProvider>, UIComponents<'static>) {
    let mut app = create_default_app();
    app.data_provide.journal_protected = true;
    app.load_entries().await.unwrap();
    let mut ui = UIComponents::new(Styles::default());
    ui.set_current_entry(Some(0), &mut app);
    (app, ui)
}

fn render<D: DataProvider>(
    ui: &mut UIComponents<'_>,
    app: &App<D>,
    width: u16,
    height: u16,
) -> Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| ui.render_ui(frame, app)).unwrap();
    terminal.backend().buffer().clone()
}

fn text(buffer: &Buffer) -> String {
    buffer.content.iter().map(|cell| cell.symbol()).collect()
}

#[tokio::test]
async fn concealment_preserves_drafts_modes_focus_selection_and_rendered_cursor() {
    for mode in [EditorMode::Normal, EditorMode::Insert, EditorMode::Visual] {
        let (mut app, mut ui) = protected_app().await;
        ui.change_active_control(ControlType::EntryContentTxt);
        ui.editor
            .set_entry_content("Private unsaved draft\nKeep my cursor and selection", &app);
        ui.editor.set_editor_mode(mode);
        ui.handle_input(&key(KeyCode::Left), &mut app)
            .await
            .unwrap();
        app.selected_entries.insert(0);
        let before = render(&mut ui, &app, 100, 30);
        let content = ui.editor.get_content();
        ui.handle_input(&control('o'), &mut app).await.unwrap();
        assert!(ui.is_details_hidden());
        assert!(app.redraw_after_restore);
        app.redraw_after_restore = false;
        for input in [
            key(KeyCode::Char('x')),
            key(KeyCode::Down),
            key(KeyCode::Tab),
            control('v'),
            control('l'),
            control('g'),
        ] {
            ui.handle_input(&input, &mut app).await.unwrap();
        }
        for command in [
            UICommand::CopyOsClipboard,
            UICommand::CutOsClipboard,
            UICommand::ExportEntryContent,
            UICommand::Undo,
        ] {
            command.execute(&mut ui, &mut app).await.unwrap();
        }
        assert_eq!(ui.editor.get_content(), content);
        assert!(ui.has_unsaved());
        assert!(!ui.take_lock_all_request());
        assert_eq!(app.current_entry_id, Some(0));
        assert!(app.selected_entries.contains(&0));
        ui.handle_input(&control('o'), &mut app).await.unwrap();
        assert!(!ui.is_details_hidden());
        assert!(app.redraw_after_restore);
        assert_eq!(render(&mut ui, &app, 100, 30), before, "mode={mode:?}");
    }
}

#[tokio::test]
async fn hidden_screen_is_independent_of_entries_filters_prompts_and_popup_text() {
    let (mut app, mut ui) = protected_app().await;
    ui.toggle_details_hidden(&mut app);
    let before = render(&mut ui, &app, 100, 30);
    app.entries[0].title = "PRIVATE_TITLE_CANARY".into();
    app.entries[0].tags = vec!["PRIVATE_TAG_CANARY".into()];
    ui.editor.set_entry_content("PRIVATE_CONTENT_CANARY", &app);
    ui.show_err_msg("PRIVATE_ERROR_CANARY with /private/path".into());
    assert_eq!(render(&mut ui, &app, 100, 30), before);
    app.entries.clear();
    app.current_entry_id = None;
    assert_eq!(render(&mut ui, &app, 100, 30), before);
    for (width, height) in [(1, 1), (5, 3), (12, 4), (80, 24), (160, 50)] {
        let screen = text(&render(&mut ui, &app, width, height));
        assert!(!screen.contains("PRIVATE"));
        assert!(!screen.contains("/private/path"));
    }
}

#[tokio::test]
async fn concealment_intercepts_popup_shortcuts_before_the_popup_and_restores_its_state() {
    for command in [
        UICommand::EditCurrentEntry,
        UICommand::ShowHelp,
        UICommand::ShowFilter,
        UICommand::ShowFuzzyFind,
        UICommand::ShowSortOptions,
    ] {
        let (mut app, mut ui) = protected_app().await;
        command.execute(&mut ui, &mut app).await.unwrap();
        assert!(ui.has_popup());
        let before = render(&mut ui, &app, 120, 40);
        let count = ui.popup_stack.len();
        ui.handle_input(&control('o'), &mut app).await.unwrap();
        assert!(ui.is_details_hidden(), "{command:?}");
        for input in [
            key(KeyCode::Enter),
            key(KeyCode::Esc),
            key(KeyCode::Char('o')),
            key(KeyCode::Char('x')),
            key(KeyCode::Tab),
        ] {
            ui.handle_input(&input, &mut app).await.unwrap();
        }
        assert_eq!(ui.popup_stack.len(), count);
        ui.handle_input(&control('o'), &mut app).await.unwrap();
        assert_eq!(render(&mut ui, &app, 120, 40), before, "{command:?}");
    }
}

#[tokio::test]
async fn concealment_covers_config_export_and_password_popups_without_consuming_input() {
    let (mut app, mut ui) = protected_app().await;
    let directory = tempfile::tempdir().unwrap();
    let mut config = ConfigPopup::new(
        crate::settings::document::ConfigDocument::load(directory.path().join("config.toml"))
            .unwrap(),
        themes::ThemeStore::from_directory(directory.path().join("themes")).unwrap(),
        &app.settings,
        &ui.styles,
        Some("tui"),
        30,
    )
    .unwrap();
    config.set_journal_protected(true);
    let export = ExportPopup::create_entry_content(app.get_current_entry().unwrap(), &app).unwrap();
    let password = journal_security_popup::JournalSecurityPopup::new(
        journal_security_popup::JournalPasswordAction::Change,
    );
    for popup in [
        Popup::Config(Box::new(config)),
        Popup::Export(Box::new(export)),
        Popup::JournalSecurity(Box::new(password)),
    ] {
        ui.popup_stack.push(popup);
        let before = render(&mut ui, &app, 120, 40);
        ui.handle_input(&control('o'), &mut app).await.unwrap();
        for input in [
            key(KeyCode::Enter),
            key(KeyCode::Char('x')),
            key(KeyCode::Esc),
        ] {
            ui.handle_input(&input, &mut app).await.unwrap();
        }
        assert!(ui.is_details_hidden());
        assert_eq!(ui.popup_stack.len(), 1);
        ui.handle_input(&control('o'), &mut app).await.unwrap();
        assert_eq!(render(&mut ui, &app, 120, 40), before);
        ui.popup_stack.pop();
    }
}

#[tokio::test]
async fn concealment_repaints_a_previously_visible_terminal_without_private_cells() {
    let (mut app, mut ui) = protected_app().await;
    ui.editor.set_entry_content("VISIBLE_BODY_CANARY", &app);
    ui.show_err_msg("VISIBLE_POPUP_CANARY".into());
    let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
    terminal.draw(|frame| ui.render_ui(frame, &app)).unwrap();
    assert!(text(terminal.backend().buffer()).contains("VISIBLE_POPUP_CANARY"));
    ui.handle_input(&control('o'), &mut app).await.unwrap();
    terminal.draw(|frame| ui.render_ui(frame, &app)).unwrap();
    let hidden = text(terminal.backend().buffer());
    assert!(!hidden.contains("CANARY"));
    assert!(hidden.contains("Show details"));
    assert!(ui.pane_split.is_none());
}

#[tokio::test]
async fn multiselect_concealment_preserves_selection_and_disables_note_security() {
    let (mut app, mut ui) = protected_app().await;
    UICommand::EnterMultiSelectMode
        .execute(&mut ui, &mut app)
        .await
        .unwrap();
    app.selected_entries.insert(0);
    ui.handle_input(&control('o'), &mut app).await.unwrap();
    ui.handle_input(&key(KeyCode::Char('x')), &mut app)
        .await
        .unwrap();
    ui.handle_input(&control('o'), &mut app).await.unwrap();
    assert!(ui.entries_list.multi_select_mode);
    assert!(app.selected_entries.contains(&0));
    for command in [
        UICommand::ShowSecurity,
        UICommand::LockNote,
        UICommand::LockAllNotes,
    ] {
        command.execute(&mut ui, &mut app).await.unwrap();
        assert!(!ui.has_popup());
        assert!(!ui.take_lock_all_request());
    }
    let screen = text(&render(&mut ui, &app, 120, 30));
    assert!(screen.contains("Hide details"));
    assert!(!screen.contains("Entry security"));
}

#[tokio::test]
async fn password_privacy_shield_cannot_be_revealed_by_ctrl_o() {
    let (mut app, mut ui) = protected_app().await;
    ui.privacy_shield = true;
    ui.handle_input(&control('o'), &mut app).await.unwrap();
    assert!(ui.privacy_shield);
    assert!(!ui.is_details_hidden());
    assert!(!text(&render(&mut ui, &app, 100, 30)).contains("Content 1"));
}

#[tokio::test]
async fn ordinary_journals_keep_note_security_and_ctrl_h_backspace_keep_editing() {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();
    let mut ui = UIComponents::new(Styles::default());
    ui.set_current_entry(Some(0), &mut app);
    ui.handle_input(&control('o'), &mut app).await.unwrap();
    assert!(!ui.is_details_hidden());
    assert!(text(&render(&mut ui, &app, 120, 30)).contains("Entry security"));
    app.data_provide.journal_protected = true;
    ui.change_active_control(ControlType::EntryContentTxt);
    ui.handle_input(&control('h'), &mut app).await.unwrap();
    assert_eq!(ui.active_control, ControlType::EntriesList);
    ui.change_active_control(ControlType::EntryContentTxt);
    ui.editor.set_entry_content("abc", &app);
    ui.editor.set_editor_mode(EditorMode::Insert);
    ui.handle_input(&key(KeyCode::End), &mut app).await.unwrap();
    ui.handle_input(&control('h'), &mut app).await.unwrap();
    ui.handle_input(&key(KeyCode::Backspace), &mut app)
        .await
        .unwrap();
    assert_eq!(ui.editor.get_content(), "a");
    assert!(!ui.is_details_hidden());
}

#[tokio::test]
async fn concealed_quit_saves_or_discards_only_after_confirmation_and_cancel_preserves_popup() {
    for save in [true, false] {
        let (mut app, mut ui) = protected_app().await;
        ui.editor.set_entry_content("DRAFT_TO_SAVE", &app);
        ui.show_err_msg("Private error".into());
        ui.toggle_details_hidden(&mut app);
        for code in [KeyCode::Char('q'), KeyCode::Esc] {
            assert_eq!(
                ui.handle_input(&key(code), &mut app).await.unwrap(),
                HandleInputReturnType::Handled
            );
        }
        assert!(ui.has_popup());
        assert!(ui.has_unsaved());
        assert!(ui.is_details_hidden());
        for code in [KeyCode::Char('q'), KeyCode::Char('y')] {
            assert_eq!(
                ui.handle_input(&key(code), &mut app).await.unwrap(),
                HandleInputReturnType::Handled
            );
        }
        assert_eq!(
            ui.handle_input(&key(KeyCode::Char(if save { 'y' } else { 'n' })), &mut app)
                .await
                .unwrap(),
            HandleInputReturnType::ExitApp
        );
        let entries = app.data_provide.load_all_entries().await.unwrap();
        assert_eq!(
            entries.iter().find(|entry| entry.id == 0).unwrap().content,
            if save { "DRAFT_TO_SAVE" } else { "Content 1" }
        );
    }
}

#[tokio::test]
async fn concealed_quit_save_failure_keeps_draft_hidden_without_displaying_backend_errors() {
    let mut app = create_default_app();
    app.data_provide.journal_protected = true;
    app.load_entries().await.unwrap();
    let mut ui = UIComponents::new(Styles::default());
    ui.set_current_entry(Some(0), &mut app);
    ui.editor.set_entry_content("PRIVATE_DRAFT_CANARY", &app);
    ui.toggle_details_hidden(&mut app);
    ui.handle_input(&key(KeyCode::Char('q')), &mut app)
        .await
        .unwrap();
    app.data_provide.set_return_err(true);
    assert_eq!(
        ui.handle_input(&key(KeyCode::Char('y')), &mut app)
            .await
            .unwrap(),
        HandleInputReturnType::Handled
    );
    assert!(ui.has_unsaved());
    assert!(ui.is_details_hidden());
    let screen = text(&render(&mut ui, &app, 120, 30));
    assert!(!screen.contains("PRIVATE_DRAFT_CANARY"));
    assert!(!screen.contains("Test Error"));
    ui.handle_input(&key(KeyCode::Enter), &mut app)
        .await
        .unwrap();
    app.data_provide.set_return_err(false);
    ui.handle_input(&key(KeyCode::Char('q')), &mut app)
        .await
        .unwrap();
    assert_eq!(
        ui.handle_input(&key(KeyCode::Char('y')), &mut app)
            .await
            .unwrap(),
        HandleInputReturnType::ExitApp
    );
}

#[tokio::test]
async fn protected_journal_requires_explicit_plaintext_export_and_clipboard_confirmation() {
    let (mut app, mut ui) = protected_app().await;
    app.settings.export.show_confirmation = false;
    for command in [
        UICommand::CopyOsClipboard,
        UICommand::CutOsClipboard,
        UICommand::ExportEntryContent,
    ] {
        command.execute(&mut ui, &mut app).await.unwrap();
        assert!(matches!(ui.popup_stack.last(), Some(Popup::MsgBox(_))));
        ui.handle_input(&key(KeyCode::Char('n')), &mut app)
            .await
            .unwrap();
        assert!(!ui.has_popup());
    }
    let error = UICommand::EditInExternalEditor
        .execute(&mut ui, &mut app)
        .await
        .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("External editing is unavailable")
    );
    ui.entries_list.multi_select_mode = true;
    app.selected_entries.insert(0);
    UICommand::MulSelExportEntries
        .execute(&mut ui, &mut app)
        .await
        .unwrap();
    assert!(matches!(ui.popup_stack.last(), Some(Popup::MsgBox(_))));
    ui.handle_input(&key(KeyCode::Char('n')), &mut app)
        .await
        .unwrap();
    assert!(!ui.has_popup());
}

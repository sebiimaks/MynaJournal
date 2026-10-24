use backend::{
    DataProvider,
    protection::{MAX_PAYLOAD_BYTES, SecretString},
};
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};

use super::security_popup::{PasswordAction, SecurityAction, SecurityPopup};
use super::*;
use crate::app::test::create_default_app;

fn key(code: KeyCode) -> Input {
    Input::new(code, KeyModifiers::NONE)
}
fn password() -> SecretString {
    "test-only passphrase 🦀".to_owned().into()
}

async fn setup() -> (App<impl DataProvider>, UIComponents<'static>) {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();
    app.protect_note(0, password(), "Private body".into())
        .await
        .unwrap();
    let mut ui = UIComponents::new(Styles::default());
    ui.set_current_entry(Some(0), &mut app);
    (app, ui)
}

fn screen<D: DataProvider>(ui: &mut UIComponents<'_>, app: &App<D>) -> String {
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| ui.render_ui(frame, app)).unwrap();
    terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect()
}

async fn submit_password<D: DataProvider>(ui: &mut UIComponents<'_>, app: &mut App<D>) {
    for ch in "test-only passphrase 🦀".chars() {
        ui.handle_input(&key(KeyCode::Char(ch)), app).await.unwrap();
    }
    ui.handle_input(&key(KeyCode::Enter), app).await.unwrap();
}

#[tokio::test]
async fn security_shortcut_opens_focused_password_field_and_protects_after_confirmation() {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();
    let mut ui = UIComponents::new(Styles::default());
    ui.set_current_entry(Some(0), &mut app);
    let original = app.get_current_entry().unwrap().clone();
    ui.handle_input(&key(KeyCode::Char('L')), &mut app)
        .await
        .unwrap();
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| ui.render_ui(frame, &app)).unwrap();
    let cursor = terminal.get_cursor_position().unwrap();
    let buffer = terminal.backend().buffer();
    let rendered: String = buffer.content().iter().map(|cell| cell.symbol()).collect();
    assert!(rendered.contains("Protect Note"));
    assert!(rendered.contains("New Password"));
    assert!(!rendered.contains("Type password here"));
    assert_eq!(buffer[(cursor.x, cursor.y)].symbol(), " ");
    assert!(rendered.contains("Enter: confirm password"));
    assert_eq!(buffer[(cursor.x - 1, cursor.y)].symbol(), "│");

    let passphrase = "P-test passphrase 🦀";
    for ch in passphrase.chars() {
        ui.handle_input(&key(KeyCode::Char(ch)), &mut app)
            .await
            .unwrap();
    }
    let rendered = screen(&mut ui, &app);
    assert!(rendered.contains("***"));
    assert!(!rendered.contains(passphrase));
    ui.handle_input(&key(KeyCode::Enter), &mut app)
        .await
        .unwrap();
    assert!(!app.current_is_protected());
    let rendered = screen(&mut ui, &app);
    assert!(rendered.contains("Confirm Password"));
    assert!(rendered.contains("Enter: protect note"));
    for ch in passphrase.chars() {
        ui.handle_input(&key(KeyCode::Char(ch)), &mut app)
            .await
            .unwrap();
    }
    ui.handle_input(&key(KeyCode::Enter), &mut app)
        .await
        .unwrap();
    assert!(!ui.has_popup());
    assert!(app.current_is_locked());
    app.unlock_note(0, passphrase.to_owned().into())
        .await
        .unwrap();
    let opened = app.get_current_entry().unwrap();
    assert_eq!(opened.content, original.content);
    assert_eq!(opened.tags, original.tags);
    assert_eq!(opened.title, original.title);
    assert_eq!(opened.date, original.date);
}

#[tokio::test]
async fn canceling_password_confirmation_preserves_note_and_unsaved_editor_text() {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();
    let mut ui = UIComponents::new(Styles::default());
    ui.set_current_entry(Some(0), &mut app);
    let original = app.get_current_entry().unwrap().clone();
    ui.editor.set_entry_content("Unsaved text", &app);
    ui.handle_input(&key(KeyCode::Char('L')), &mut app)
        .await
        .unwrap();
    ui.handle_input(&key(KeyCode::Char('x')), &mut app)
        .await
        .unwrap();
    ui.handle_input(&key(KeyCode::Enter), &mut app)
        .await
        .unwrap();
    assert!(screen(&mut ui, &app).contains("Confirm Password"));
    ui.handle_input(&key(KeyCode::Esc), &mut app).await.unwrap();
    assert!(!ui.has_popup());
    assert_eq!(app.get_current_entry(), Some(&original));
    assert_eq!(ui.editor.get_content(), "Unsaved text");
    assert!(ui.has_unsaved());
}

#[test]
fn password_field_and_cursor_remain_visible_in_compact_terminals() {
    let mut popup = SecurityPopup::new(0, false, false, false);
    popup.prompt(PasswordAction::Protect);
    for (width, height) in [(80, 24), (60, 14), (40, 10), (36, 8)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| popup.render_widget(frame, frame.area(), &Styles::default()))
            .unwrap();
        let cursor = terminal.get_cursor_position().unwrap();
        assert!(cursor.x < width && cursor.y < height);
        let buffer = terminal.backend().buffer();
        assert_eq!(buffer[(cursor.x - 1, cursor.y)].symbol(), "│");
        let rendered: String = buffer.content().iter().map(|cell| cell.symbol()).collect();
        assert!(rendered.contains("New Password"));
        assert!(!rendered.contains("Type password here"));
        assert_eq!(buffer[(cursor.x, cursor.y)].symbol(), " ");
        assert!(rendered.contains("Enter:"));
        assert!(
            rendered.contains("Esc: cancel"),
            "{width}x{height}: {rendered}"
        );
    }
    for size in [(0, 0), (1, 1), (8, 4)] {
        let mut terminal = Terminal::new(TestBackend::new(size.0, size.1)).unwrap();
        terminal
            .draw(|frame| popup.render_widget(frame, frame.area(), &Styles::default()))
            .unwrap();
    }
}

#[test]
fn security_menu_enter_opens_primary_action_without_hidden_letter_shortcut() {
    let mut popup = SecurityPopup::new(0, false, false, false);
    assert!(matches!(
        popup.handle_input(&key(KeyCode::Enter)),
        SecurityAction::Keep
    ));
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal
        .draw(|frame| popup.render_widget(frame, frame.area(), &Styles::default()))
        .unwrap();
    let rendered: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(rendered.contains("New Password"));
    let mut open = SecurityPopup::new(0, true, false, false);
    assert!(matches!(
        open.handle_input(&key(KeyCode::Enter)),
        SecurityAction::Lock
    ));
}

#[tokio::test]
async fn locked_editor_rejects_edit_modes_and_private_commands_require_unlock() {
    let (mut app, mut ui) = setup().await;
    let rendered = screen(&mut ui, &app);
    assert!(rendered.contains("Title 1"));
    assert!(rendered.contains("Locked note"));
    assert!(!rendered.contains("Private body"));
    assert!(!rendered.contains("Tag 1"));
    ui.editor.set_editor_mode(EditorMode::Insert);
    assert_eq!(ui.editor.get_editor_mode(), EditorMode::Normal);
    ui.editor
        .handle_input_prioritized(&key(KeyCode::Char('x')), &app)
        .unwrap();
    assert_eq!(ui.editor.get_content(), "");
    for command in [
        UICommand::StartEditEntryContent,
        UICommand::EditCurrentEntry,
        UICommand::SaveEntryContent,
        UICommand::ExportEntryContent,
        UICommand::CopyOsClipboard,
        UICommand::CutOsClipboard,
        UICommand::PasteOsClipboard,
    ] {
        command.execute(&mut ui, &mut app).await.unwrap();
        assert!(matches!(ui.popup_stack.pop(), Some(Popup::Security(_))));
    }
    assert!(
        UICommand::EditInExternalEditor
            .execute(&mut ui, &mut app)
            .await
            .is_err()
    );
    assert!(app.current_is_locked());
}

#[tokio::test]
async fn idle_lock_seals_unsaved_text_and_requires_explicit_resolution_before_exit_or_delete() {
    let (mut app, mut ui) = setup().await;
    app.unlock_note(0, password()).await.unwrap();
    ui.editor.set_current_entry(Some(0), &app);
    ui.editor.set_entry_content("Private unsaved draft", &app);
    ui.hide_for_auto_lock();
    assert!(!screen(&mut ui, &app).contains("Private unsaved draft"));
    ui.auto_lock(&mut app).await.unwrap();
    assert!(app.current_is_locked());
    assert!(app.has_sealed_draft(0));
    assert_eq!(ui.editor.get_content(), "");
    assert!(UICommand::Quit.execute(&mut ui, &mut app).await.is_err());
    assert!(app.delete_entry(0).await.is_err());
    assert!(app.load_entries().await.is_err());
    UICommand::StartEditEntryContent
        .execute(&mut ui, &mut app)
        .await
        .unwrap();
    submit_password(&mut ui, &mut app).await;
    assert_eq!(ui.editor.get_content(), "Private unsaved draft");
    assert!(ui.has_unsaved());
    assert!(!app.has_sealed_drafts());
    assert_eq!(app.get_current_entry().unwrap().content, "Private body");
    UICommand::SaveEntryContent
        .execute(&mut ui, &mut app)
        .await
        .unwrap();
    assert!(!ui.has_unsaved());
    ui.auto_lock(&mut app).await.unwrap();
    assert!(!app.has_sealed_drafts());
    assert_eq!(app.unlock_note(0, password()).await.unwrap(), None);
    assert_eq!(
        app.get_current_entry().unwrap().content,
        "Private unsaved draft"
    );
}

#[tokio::test]
async fn failed_idle_lock_hides_and_retains_draft_until_password_is_verified() {
    let (mut app, mut ui) = setup().await;
    app.unlock_note(0, password()).await.unwrap();
    ui.editor.set_current_entry(Some(0), &app);
    let oversized = "s".repeat(MAX_PAYLOAD_BYTES + 1);
    ui.editor.set_entry_content(&oversized, &app);
    ui.hide_for_auto_lock();
    let error = ui.auto_lock(&mut app).await.unwrap_err();
    ui.recover_failed_auto_lock(&mut app, &error).unwrap();
    assert!(ui.is_privacy_shielded());
    assert!(screen(&mut ui, &app).contains("Could not seal"));
    ui.handle_input(&key(KeyCode::Esc), &mut app).await.unwrap();
    ui.handle_input(&key(KeyCode::Char('q')), &mut app)
        .await
        .unwrap();
    assert!(ui.is_privacy_shielded());
    assert_eq!(ui.editor.get_content(), oversized);
    ui.handle_input(&key(KeyCode::Enter), &mut app)
        .await
        .unwrap();
    ui.handle_input(&key(KeyCode::Char('x')), &mut app)
        .await
        .unwrap();
    ui.handle_input(&key(KeyCode::Enter), &mut app)
        .await
        .unwrap();
    assert!(ui.is_privacy_shielded());
    submit_password(&mut ui, &mut app).await;
    assert!(!ui.is_privacy_shielded());
    assert!(ui.has_unsaved());
    assert_eq!(ui.editor.get_content(), oversized);
    assert_eq!(app.get_current_entry().unwrap().content, "Private body");
}

#[tokio::test]
async fn locking_background_sessions_preserves_unsaved_plaintext_note_and_popup() {
    let (mut app, mut ui) = setup().await;
    app.settings.security.lock_on_leave = false;
    app.unlock_note(0, password()).await.unwrap();
    ui.set_current_entry(Some(1), &mut app);
    ui.editor.set_entry_content("Ordinary unsaved note", &app);
    UICommand::ShowHelp
        .execute(&mut ui, &mut app)
        .await
        .unwrap();
    ui.hide_for_auto_lock();
    ui.auto_lock(&mut app).await.unwrap();
    assert!(app.is_locked(0));
    assert!(!ui.is_privacy_shielded());
    assert!(matches!(ui.popup_stack.last(), Some(Popup::Help(_))));
    assert_eq!(ui.editor.get_content(), "Ordinary unsaved note");
    assert!(ui.has_unsaved());
}

#[tokio::test]
async fn private_clipboard_and_plaintext_export_require_explicit_confirmation() {
    let (mut app, mut ui) = setup().await;
    app.unlock_note(0, password()).await.unwrap();
    ui.editor.set_current_entry(Some(0), &app);
    ui.change_active_control(ControlType::EntryContentTxt);
    for command in [
        UICommand::CopyOsClipboard,
        UICommand::CutOsClipboard,
        UICommand::ExportEntryContent,
    ] {
        command.execute(&mut ui, &mut app).await.unwrap();
        assert!(matches!(ui.popup_stack.last(), Some(Popup::MsgBox(_))));
        assert!(
            screen(&mut ui, &app).contains(if command == UICommand::ExportEntryContent {
                "plaintext"
            } else {
                "clipboard history"
            })
        );
        ui.handle_input(&key(KeyCode::Char('n')), &mut app)
            .await
            .unwrap();
        assert!(!ui.has_popup());
        assert_eq!(ui.editor.get_content(), "Private body");
    }
    assert!(
        UICommand::EditInExternalEditor
            .execute(&mut ui, &mut app)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn idle_lock_restores_unfinished_metadata_form_without_saving_it() {
    let (mut app, mut ui) = setup().await;
    app.unlock_note(0, password()).await.unwrap();
    ui.editor.set_current_entry(Some(0), &app);
    UICommand::EditCurrentEntry
        .execute(&mut ui, &mut app)
        .await
        .unwrap();
    ui.handle_input(&key(KeyCode::Char('X')), &mut app)
        .await
        .unwrap();
    let before = match ui.popup_stack.last().unwrap() {
        Popup::Entry(popup) => popup.snapshot().unwrap(),
        _ => panic!("metadata form expected"),
    };
    ui.hide_for_auto_lock();
    ui.auto_lock(&mut app).await.unwrap();
    assert!(app.has_sealed_draft(0));
    assert!(!ui.has_popup());
    assert_eq!(app.get_current_entry().unwrap().title, "Title 1");
    UICommand::StartEditEntryContent
        .execute(&mut ui, &mut app)
        .await
        .unwrap();
    submit_password(&mut ui, &mut app).await;
    let after = match ui.popup_stack.last().unwrap() {
        Popup::Entry(popup) => popup.snapshot().unwrap(),
        _ => panic!("metadata form must be restored after unlock"),
    };
    assert_eq!(after, before);
    assert!(!ui.has_unsaved());
    assert_eq!(app.get_current_entry().unwrap().title, "Title 1");
}

#[tokio::test]
async fn bulk_delete_checks_all_encrypted_drafts_before_deleting_any_selected_note() {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();
    app.selected_entries.extend([0, 1]);
    let protected_id = *app.selected_entries.iter().last().unwrap();
    app.protect_note(protected_id, password(), "Private saved body".into())
        .await
        .unwrap();
    app.unlock_note(protected_id, password()).await.unwrap();
    app.seal_draft_and_lock(protected_id, "Private unsaved draft".into(), None)
        .await
        .unwrap();
    let before: Vec<_> = app.get_active_entries().cloned().collect();
    let mut ui = UIComponents::new(Styles::default());
    let error = UICommand::MulSelDeleteEntries
        .continue_executing(&mut ui, &mut app, MsgBoxResult::Yes)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("encrypted drafts"));
    assert_eq!(
        app.get_active_entries().cloned().collect::<Vec<_>>(),
        before
    );
    assert_eq!(app.selected_entries.len(), 2);
    assert!(app.has_sealed_draft(protected_id));

    let draft = app
        .unlock_note(protected_id, password())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(draft.content, "Private unsaved draft");
}

#[tokio::test]
async fn leaving_protected_note_locks_it_and_navigation_preserves_plaintext_undo_history() {
    let (mut app, mut ui) = setup().await;
    app.unlock_note(0, password()).await.unwrap();
    ui.editor.set_current_entry(Some(0), &app);
    ui.set_current_entry(Some(1), &mut app);
    assert!(app.is_locked(0));
    app.update_current_entry_content("Changed ordinary note".into())
        .await
        .unwrap();
    ui.set_current_entry(Some(0), &mut app);
    ui.set_current_entry(Some(1), &mut app);
    app.undo().await.unwrap();
    assert_eq!(app.get_current_entry().unwrap().content, "Content 2");
}

#[tokio::test]
async fn external_editor_attempt_locks_background_notes_before_waiting_for_process() {
    let (mut app, mut ui) = setup().await;
    app.settings.security.lock_on_leave = false;
    app.unlock_note(0, password()).await.unwrap();
    ui.set_current_entry(Some(1), &mut app);
    app.settings.external_editor.command = Some(String::new());
    let error = UICommand::EditInExternalEditor
        .execute(&mut ui, &mut app)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("is empty"));
    assert!(app.is_locked(0));
    assert_eq!(ui.editor.get_content(), "Content 2");
}

#[test]
fn password_prompt_masks_unicode_requires_confirmation_and_does_not_accept_controls() {
    use backend::protection::ExposeSecret;
    let mut popup = SecurityPopup::new(0, false, false, false);
    popup.prompt(PasswordAction::Protect);
    for ch in "秘密🦀".chars() {
        popup.handle_input(&key(KeyCode::Char(ch)));
    }
    popup.handle_input(&Input::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal
        .draw(|f| popup.render_widget(f, f.area(), &Styles::default()))
        .unwrap();
    let rendered: String = terminal
        .backend()
        .buffer()
        .content()
        .iter()
        .map(|c| c.symbol())
        .collect();
    assert!(!rendered.contains("秘密"));
    assert!(rendered.contains("***"));
    assert!(matches!(
        popup.handle_input(&key(KeyCode::Enter)),
        SecurityAction::Keep
    ));
    popup.handle_input(&key(KeyCode::Char('x')));
    assert!(matches!(
        popup.handle_input(&key(KeyCode::Enter)),
        SecurityAction::Keep
    ));
    for ch in "秘密🦀".chars() {
        popup.handle_input(&key(KeyCode::Char(ch)));
    }
    match popup.handle_input(&key(KeyCode::Enter)) {
        SecurityAction::Protect(password) => assert_eq!(password.expose_secret(), "秘密🦀"),
        _ => panic!("confirmed password should be submitted"),
    }
}

use backend::DataProvider;
use crossterm::event::{KeyCode, KeyModifiers};

use super::*;
use crate::app::{HandleInputReturnType, test::create_default_app};

const DRAFT: &str = "Unsaved changes before returning to the journal selector";

fn key(code: KeyCode) -> Input {
    Input::new(code, KeyModifiers::NONE)
}

async fn with_draft<D: DataProvider>(app: &mut App<D>) -> UIComponents<'static> {
    app.load_entries().await.unwrap();
    let mut ui = UIComponents::new(Styles::default());
    ui.set_current_entry(Some(0), app);
    ui.editor.set_entry_content(DRAFT, app);
    assert!(ui.has_unsaved());
    ui
}

async fn persisted_content<D: DataProvider>(app: &mut App<D>) -> String {
    app.data_provide
        .load_all_entries()
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.id == 0)
        .unwrap()
        .content
}

#[tokio::test]
async fn leaving_journal_saves_the_draft_before_signalling_exit() {
    let mut app = create_default_app();
    let mut ui = with_draft(&mut app).await;
    assert_eq!(
        ui.handle_input(&key(KeyCode::Char('q')), &mut app)
            .await
            .unwrap(),
        HandleInputReturnType::Handled
    );
    assert!(matches!(ui.popup_stack.last(), Some(Popup::MsgBox(_))));
    assert_eq!(
        ui.handle_input(&key(KeyCode::Char('y')), &mut app)
            .await
            .unwrap(),
        HandleInputReturnType::ExitApp
    );
    assert!(!ui.has_unsaved());
    assert!(!ui.has_popup());
    assert_eq!(persisted_content(&mut app).await, DRAFT);
}

#[tokio::test]
async fn leaving_journal_cancel_keeps_the_draft_and_discard_keeps_the_saved_note() {
    for cancel in [KeyCode::Esc, KeyCode::Char('c')] {
        let mut app = create_default_app();
        let mut ui = with_draft(&mut app).await;
        let original = app.get_current_entry().unwrap().content.clone();
        for code in [KeyCode::Char('q'), KeyCode::Char('q'), cancel] {
            assert_eq!(
                ui.handle_input(&key(code), &mut app).await.unwrap(),
                HandleInputReturnType::Handled
            );
        }
        assert!(!ui.has_popup());
        assert!(ui.has_unsaved());
        assert_eq!(ui.editor.get_content(), DRAFT);
        assert_eq!(persisted_content(&mut app).await, original);
        assert_eq!(
            ui.handle_input(&key(KeyCode::Char('q')), &mut app)
                .await
                .unwrap(),
            HandleInputReturnType::Handled
        );
        assert_eq!(
            ui.handle_input(&key(KeyCode::Char('n')), &mut app)
                .await
                .unwrap(),
            HandleInputReturnType::ExitApp
        );
        assert_eq!(persisted_content(&mut app).await, original);
    }
}

#[tokio::test]
async fn failed_save_keeps_the_journal_open_until_retry_or_explicit_discard() {
    for retry in [true, false] {
        let mut app = create_default_app();
        let mut ui = with_draft(&mut app).await;
        let original = app.get_current_entry().unwrap().content.clone();
        assert_eq!(
            ui.handle_input(&key(KeyCode::Char('q')), &mut app)
                .await
                .unwrap(),
            HandleInputReturnType::Handled
        );
        app.data_provide.set_return_err(true);
        let error = ui
            .handle_input(&key(KeyCode::Char('y')), &mut app)
            .await
            .unwrap_err();
        assert!(ui.has_unsaved());
        assert_eq!(ui.editor.get_content(), DRAFT);
        assert_eq!(app.get_current_entry().unwrap().content, original);
        ui.show_err_msg(error.to_string());
        assert_eq!(
            ui.handle_input(&key(KeyCode::Enter), &mut app)
                .await
                .unwrap(),
            HandleInputReturnType::Handled
        );
        app.data_provide.set_return_err(false);
        assert_eq!(persisted_content(&mut app).await, original);
        assert_eq!(
            ui.handle_input(&key(KeyCode::Char('q')), &mut app)
                .await
                .unwrap(),
            HandleInputReturnType::Handled
        );
        assert_eq!(
            ui.handle_input(&key(KeyCode::Char(if retry { 'y' } else { 'n' })), &mut app)
                .await
                .unwrap(),
            HandleInputReturnType::ExitApp
        );
        assert_eq!(
            persisted_content(&mut app).await,
            if retry { DRAFT } else { &original }
        );
    }
}

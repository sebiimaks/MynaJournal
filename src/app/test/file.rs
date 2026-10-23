use std::fs;

use backend::{FileDataProvider, FileFormat};
use tempfile::TempDir;

use super::*;

#[tokio::test]
async fn file_history_preserves_identity_metadata_and_retryable_restore_conflicts() {
    let directory = TempDir::new().unwrap();
    let provider = FileDataProvider::new(directory.path().to_owned(), FileFormat::Plain);
    let mut app = App::new(provider, Settings::default());
    app.load_entries().await.unwrap();
    let id = app
        .add_entry(
            "Journal history".into(),
            Utc.with_ymd_and_hms(2026, 10, 23, 7, 8, 9).unwrap(),
            vec!["personal".into()],
            Some(2),
        )
        .await
        .unwrap();
    app.current_entry_id = Some(id);
    let path = fs::read_dir(directory.path())
        .unwrap()
        .map(|item| item.unwrap().path())
        .find(|path| !path.file_name().unwrap().to_string_lossy().starts_with('.'))
        .unwrap();
    let header = fs::read_to_string(&path).unwrap().replacen(
        "---\n",
        "---\n# Keep this comment\ncustom:\n  source: manuscript\n  flags: [one, 2, true]\n",
        1,
    );
    fs::write(&path, &header).unwrap();
    app.load_entries().await.unwrap();

    let content = "Edited journal 世界\r\nTrailing spaces  \n";
    app.update_current_entry_content(content.into())
        .await
        .unwrap();
    let edited = app.get_entry(id).unwrap().clone();
    let saved = format!("{header}{content}");
    assert_eq!(fs::read_to_string(&path).unwrap(), saved);
    app.delete_entry(id).await.unwrap();
    assert!(!path.exists());

    fs::write(&path, "Occupied restore destination").unwrap();
    assert!(app.undo().await.is_err());
    assert!(app.get_entry(id).is_none());
    assert_eq!(
        fs::read_to_string(&path).unwrap(),
        "Occupied restore destination"
    );
    fs::remove_file(&path).unwrap();

    assert_eq!(app.undo().await.unwrap(), Some(id));
    assert_eq!(app.get_entry(id), Some(&edited));
    assert_eq!(fs::read_to_string(&path).unwrap(), saved);
    assert_eq!(app.undo().await.unwrap(), Some(id));
    assert!(app.get_entry(id).unwrap().content.is_empty());
    assert_eq!(fs::read_to_string(&path).unwrap(), header);

    assert_eq!(app.redo().await.unwrap(), Some(id));
    assert_eq!(app.get_entry(id), Some(&edited));
    assert_eq!(fs::read_to_string(&path).unwrap(), saved);
    assert!(app.redo().await.unwrap().is_none());
    assert!(!path.exists());
    assert_eq!(app.undo().await.unwrap(), Some(id));
    assert_eq!(fs::read_to_string(&path).unwrap(), saved);
    app.load_entries().await.unwrap();
    assert_eq!(app.get_entry(id), Some(&edited));
}

#[tokio::test]
async fn saving_after_reloading_an_externally_deleted_last_file_returns_an_error() {
    use crossterm::event::{KeyCode, KeyModifiers};
    use ui::Styles;

    let directory = TempDir::new().unwrap();
    let provider = FileDataProvider::new(directory.path().to_owned(), FileFormat::Plain);
    let mut app = App::new(provider, Settings::default());
    app.load_entries().await.unwrap();
    let id = app
        .add_entry("Last journal".into(), Utc::now(), vec![], None)
        .await
        .unwrap();
    let mut ui = UIComponents::new(Styles::default());
    ui.set_current_entry(Some(id), &mut app);
    let path = fs::read_dir(directory.path())
        .unwrap()
        .map(|item| item.unwrap().path())
        .find(|path| !path.file_name().unwrap().to_string_lossy().starts_with('.'))
        .unwrap();
    fs::remove_file(&path).unwrap();

    ui.handle_input(
        &keymap::Input::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
        &mut app,
    )
    .await
    .unwrap();
    ui.update_current_entry(&mut app);
    assert!(app.entries.is_empty());
    assert_eq!(app.current_entry_id, None);
    assert!(!ui.has_unsaved());

    let error = ui
        .handle_input(
            &keymap::Input::new(KeyCode::Char('s'), KeyModifiers::NONE),
            &mut app,
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("No journal is selected"));
    let error = app
        .update_current_entry_attributes("Changed".into(), Utc::now(), vec![], None)
        .await
        .unwrap_err();
    assert!(error.to_string().contains("No journal is selected"));
    assert!(!ui.has_unsaved());
    assert!(app.entries.is_empty());
    assert!(!path.exists());
    assert!(matches!(
        app.history.pop_undo(),
        Some(Change::AddEntry { id: previous }) if previous == id
    ));
    assert!(app.history.pop_undo().is_none());
    assert!(app.history.pop_redo().is_none());
}

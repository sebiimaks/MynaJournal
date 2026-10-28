use super::*;
use crate::app::test::create_default_app;

fn key(code: KeyCode) -> Input {
    Input::new(code, KeyModifiers::NONE)
}

#[tokio::test]
async fn suggested_names_cannot_escape_existing_directories_and_remain_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let dotted = dir.path().join("exports.v1");
    std::fs::create_dir(&dotted).unwrap();
    let mut app = create_default_app();
    app.load_entries().await.unwrap();
    app.settings.export.default_path = Some(dotted.clone());
    let mut entry = app.entries[0].clone();
    for title in [
        "../victim".to_owned(),
        "/tmp/victim".to_owned(),
        "a\\b\n\0:c".to_owned(),
        String::new(),
        "🦀".repeat(500),
    ] {
        entry.title = title;
        let mut popup = ExportPopup::create_entry_content(&entry, &app).unwrap();
        let PopupReturn::Apply((target, _)) = popup.handle_input(&key(KeyCode::Enter)) else {
            panic!("new export should be accepted");
        };
        assert_eq!(
            target.path().parent().unwrap(),
            std::fs::canonicalize(&dotted).unwrap()
        );
        assert!(target.path().file_name().unwrap().len() < 200);
    }
}

#[tokio::test]
async fn existing_files_require_explicit_replacement_and_keep_the_original_on_cancel() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("existing-no-extension");
    std::fs::write(&path, "keep").unwrap();
    let mut app = create_default_app();
    app.load_entries().await.unwrap();
    app.settings.export.default_path = Some(path.clone());
    let mut popup = ExportPopup::create_entry_content(&app.entries[0], &app).unwrap();
    assert!(matches!(
        popup.handle_input(&key(KeyCode::Enter)),
        PopupReturn::KeepPopup
    ));
    assert!(popup.replacement.is_some());
    assert!(matches!(
        popup.handle_input(&key(KeyCode::Enter)),
        PopupReturn::KeepPopup
    ));
    assert!(matches!(
        popup.handle_input(&key(KeyCode::Char('n'))),
        PopupReturn::KeepPopup
    ));
    assert!(popup.replacement.is_none());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "keep");
    assert!(matches!(
        popup.handle_input(&key(KeyCode::Enter)),
        PopupReturn::KeepPopup
    ));
    let PopupReturn::Apply((target, _)) = popup.handle_input(&key(KeyCode::Char('y'))) else {
        panic!("explicit replacement should be accepted");
    };
    assert_eq!(target.path(), std::fs::canonicalize(path).unwrap());
    assert!(target.replaces_file());
}

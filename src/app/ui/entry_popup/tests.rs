use super::*;
use crate::app::test::create_default_app;

#[tokio::test]
async fn editing_metadata_preserves_the_full_original_timestamp() {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();
    app.current_entry_id = Some(1);
    let timestamp = "2026-10-23T19:42:18.123456789+10:00"
        .parse::<DateTime<chrono::FixedOffset>>()
        .unwrap()
        .with_timezone(&Utc);
    app.update_current_entry_attributes("Imported".into(), timestamp, vec![], None)
        .await
        .unwrap();

    let mut popup = EntryPopup::from_entry(app.get_current_entry().unwrap());
    popup.title_txt = TextArea::new(vec!["Edited title".into()]);
    popup.tags_txt = TextArea::new(vec!["first, second".into()]);
    popup.priority_txt = TextArea::new(vec!["3".into()]);
    assert_eq!(
        popup.handle_confirm(&mut app).await.unwrap(),
        EntryPopupInputReturn::UpdateCurrentEntry
    );

    let entry = app.get_current_entry().unwrap();
    assert_eq!(entry.date, timestamp);
    assert_eq!(entry.title, "Edited title");
    assert_eq!(entry.tags, ["first", "second"]);
    assert_eq!(entry.priority, Some(3));
    app.load_entries().await.unwrap();
    assert_eq!(app.get_current_entry().unwrap().date, timestamp);
}

#[tokio::test]
async fn changing_the_calendar_date_uses_midnight_on_the_new_day() {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();
    app.current_entry_id = Some(1);
    let mut popup = EntryPopup::from_entry(app.get_current_entry().unwrap());
    popup.date_txt = TextArea::new(vec!["23-10-2026".into()]);
    popup.handle_confirm(&mut app).await.unwrap();
    assert_eq!(
        app.get_current_entry().unwrap().date,
        Utc.with_ymd_and_hms(2026, 10, 23, 0, 0, 0).unwrap()
    );
}

#[tokio::test]
async fn failed_metadata_save_can_retry_without_losing_the_timestamp() {
    let mut app = create_default_app();
    app.load_entries().await.unwrap();
    app.current_entry_id = Some(1);
    let original = app.get_current_entry().unwrap().clone();
    let mut popup = EntryPopup::from_entry(&original);
    popup.title_txt = TextArea::new(vec!["Retry title".into()]);
    app.data_provide.set_return_err(true);
    assert!(popup.handle_confirm(&mut app).await.is_err());
    assert_eq!(app.get_current_entry(), Some(&original));

    app.data_provide.set_return_err(false);
    popup.handle_confirm(&mut app).await.unwrap();
    let entry = app.get_current_entry().unwrap();
    assert_eq!(entry.title, "Retry title");
    assert_eq!(entry.date, original.date);
}

#[tokio::test]
async fn new_entries_keep_auto_title_and_calendar_date_behavior() {
    let mut app = create_default_app();
    app.settings = toml::from_str("auto_title = 'Daily journal'").unwrap();
    let mut popup = EntryPopup::new_entry(&app.settings);
    assert_eq!(popup.title_txt.lines(), ["Daily journal"]);
    popup.date_txt = TextArea::new(vec!["23-10-2026".into()]);
    let EntryPopupInputReturn::AddEntry(id) = popup.handle_confirm(&mut app).await.unwrap() else {
        panic!("valid new entry was not added");
    };
    let entry = app.get_entry(id).unwrap();
    assert_eq!(entry.title, "Daily journal");
    assert_eq!(
        entry.date,
        Utc.with_ymd_and_hms(2026, 10, 23, 0, 0, 0).unwrap()
    );
}

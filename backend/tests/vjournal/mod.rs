use std::path::PathBuf;

use backend::*;
use chrono::{TimeZone, Utc};
use tempfile::{Builder, TempDir};
use vobject::{Component, parse_component};

#[tokio::test]
async fn delete_and_undo_restore_the_native_recurrence_family_and_preserve_other_entries() {
    for keep_other_components in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let mut original = parse_component(RECURRING_CALENDAR).unwrap();
        if !keep_other_components {
            original.subcomponents.retain(|component| {
                component.name == "VJOURNAL"
                    && component_uid(component) == Some("recurring-journal")
            });
        }
        original
            .subcomponents
            .iter_mut()
            .find(|component| component.get_only("RRULE").is_some())
            .unwrap()
            .set(vobject::Property::new("X-PRIVATE-CUSTOM", "native value"));
        let path = write_calendar(&dir, &vobject::component::write_component(&original)).await;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        }
        let family = |calendar: &Component| {
            calendar
                .subcomponents
                .iter()
                .filter(|component| {
                    component.name == "VJOURNAL"
                        && component_uid(component) == Some("recurring-journal")
                })
                .map(vobject::component::write_component)
                .collect::<Vec<_>>()
        };
        let expected = family(&original);
        let mut provider = create_provider(&dir);
        let entries = provider.load_all_entries().await.unwrap();
        let removed = entries
            .iter()
            .find(|entry| entry.title == "Series master")
            .unwrap()
            .clone();
        provider.remove_entry(removed.id).await.unwrap();
        if keep_other_components {
            let mut other = entries
                .iter()
                .find(|entry| entry.title == "Standalone")
                .unwrap()
                .clone();
            other.content = "changed while family was deleted".into();
            provider.update_entry(other).await.unwrap();
        } else {
            assert!(!path.exists());
        }
        provider.restore_entry(removed).await.unwrap();
        let restored = parse_component(&std::fs::read_to_string(&path).unwrap()).unwrap();
        assert_eq!(family(&restored), expected);
        if keep_other_components {
            assert!(restored.subcomponents.iter().any(|component| {
                component.get_only("DESCRIPTION").is_some_and(|value| {
                    value.value_as_string() == "changed while family was deleted"
                })
            }));
            assert!(
                restored
                    .subcomponents
                    .iter()
                    .any(|component| component.name == "VEVENT")
            );
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o640
            );
        }
    }
}

#[tokio::test]
async fn restore_refuses_an_occupied_path_and_keeps_metadata_for_retry() {
    let dir = tempfile::tempdir().unwrap();
    let mut provider = create_provider(&dir);
    let entry = provider
        .add_entry(EntryDraft::new(Utc::now(), "restore".into(), vec![], None))
        .await
        .unwrap();
    let path = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.extension().is_some_and(|extension| extension == "ics"))
        .unwrap();
    let original = std::fs::read(&path).unwrap();
    provider.remove_entry(entry.id).await.unwrap();
    std::fs::write(&path, "unrelated data").unwrap();
    assert!(provider.restore_entry(entry.clone()).await.is_err());
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "unrelated data");
    std::fs::remove_file(&path).unwrap();
    provider.restore_entry(entry).await.unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[tokio::test]
async fn undo_keeps_detached_recurrences_in_other_calendar_files() {
    let dir = TempDir::new().unwrap();
    let mut original = parse_component(RECURRING_CALENDAR).unwrap();
    original.subcomponents.retain(|component| {
        component.name == "VJOURNAL" && component_uid(component) == Some("recurring-journal")
    });
    let detached = original.subcomponents.remove(0);
    let master_path = write_calendar(&dir, &vobject::component::write_component(&original)).await;
    let mut other = original.clone();
    other.subcomponents = vec![detached];
    let detached_path = dir.path().join("detached.ics");
    let detached_text = vobject::component::write_component(&other);
    std::fs::write(&detached_path, &detached_text).unwrap();
    let mut provider = create_provider(&dir);
    let entry = provider.load_all_entries().await.unwrap().remove(0);
    provider.remove_entry(entry.id).await.unwrap();
    assert!(!master_path.exists());
    provider.restore_entry(entry.clone()).await.unwrap();
    assert_eq!(provider.load_all_entries().await.unwrap(), vec![entry]);
    assert_eq!(
        std::fs::read_to_string(detached_path).unwrap(),
        detached_text
    );
}

fn create_provider(dir: &TempDir) -> VjournalDataProvide {
    VjournalDataProvide::new(dir.path().to_path_buf())
}

async fn write_calendar(dir: &TempDir, content: &str) -> PathBuf {
    let path = dir.path().join("calendar.ics");
    tokio::fs::write(&path, content).await.unwrap();
    path
}

fn component_uid(component: &Component) -> Option<&str> {
    component
        .get_only("UID")
        .map(|property| property.raw_value.as_str())
}

const RECURRING_CALENDAR: &str = "\
BEGIN:VCALENDAR\r
VERSION:2.0\r
PRODID:-//test//EN\r
BEGIN:VJOURNAL\r
UID:recurring-journal\r
DTSTAMP:20250101T000000Z\r
DTSTART;VALUE=DATE:20250102\r
RECURRENCE-ID;VALUE=DATE:20250102\r
SUMMARY:Detached instance\r
DESCRIPTION:Detached content\r
END:VJOURNAL\r
BEGIN:VJOURNAL\r
UID:recurring-journal\r
DTSTAMP:20250101T000000Z\r
DTSTART;VALUE=DATE:20250101\r
RRULE:FREQ=DAILY;COUNT=3\r
SUMMARY:Series master\r
DESCRIPTION:Master content\r
END:VJOURNAL\r
BEGIN:VJOURNAL\r
UID:standalone-journal\r
DTSTAMP:20250101T000000Z\r
DTSTART;VALUE=DATE:20250104\r
SUMMARY:Standalone\r
END:VJOURNAL\r
BEGIN:VEVENT\r
UID:recurring-journal\r
DTSTAMP:20250101T000000Z\r
DTSTART:20250101T120000Z\r
SUMMARY:Event with matching UID\r
END:VEVENT\r
END:VCALENDAR\r
";

async fn create_provider_with_two_entries(dir: &TempDir) -> VjournalDataProvide {
    let mut provider = create_provider(dir);

    let mut entry_draft_1 = EntryDraft::new(
        Utc::now(),
        String::from("Title 1"),
        vec![String::from("Tag_1"), String::from("Tag_2")],
        None,
    );
    entry_draft_1.content.push_str("Content entry 1");
    let mut entry_draft_2 = EntryDraft::new(
        Utc.with_ymd_and_hms(2023, 3, 23, 1, 1, 1).unwrap(),
        String::from("Title 2"),
        Vec::new(),
        Some(1),
    );
    entry_draft_2.content.push_str("Content entry 2");

    provider.add_entry(entry_draft_1).await.unwrap();
    provider.add_entry(entry_draft_2).await.unwrap();

    provider
}

#[tokio::test]
async fn create_provider_with_default_entries() {
    let dir = Builder::new().prefix("vj-defaults").tempdir().unwrap();
    let mut provider = create_provider_with_two_entries(&dir).await;

    let entries = provider.load_all_entries().await.unwrap();

    assert_eq!(entries.len(), 2);
    // IDs are assigned sequentially; look up by title since file ordering is
    // non-deterministic (UUID-based filenames).
    let ids: Vec<u32> = entries.iter().map(|e| e.id).collect();
    assert!(ids.contains(&0));
    assert!(ids.contains(&1));

    let entry1 = entries.iter().find(|e| e.title == "Title 1").unwrap();
    let entry2 = entries.iter().find(|e| e.title == "Title 2").unwrap();
    assert_eq!(entry1.priority, None);
    assert_eq!(entry2.priority, Some(1));
}

#[tokio::test]
async fn add_entry() {
    let dir = Builder::new().prefix("vj-add").tempdir().unwrap();
    let mut provider = create_provider_with_two_entries(&dir).await;

    let mut entry_draft = EntryDraft::new(
        Utc.with_ymd_and_hms(2023, 3, 23, 1, 1, 1).unwrap(),
        String::from("Title added"),
        vec![String::from("Tag_1"), String::from("Tag_3")],
        Some(1),
    );
    entry_draft.content.push_str("Content entry added");

    let added_entry = provider.add_entry(entry_draft).await.unwrap();

    let expected_date = Utc.with_ymd_and_hms(2023, 3, 23, 0, 0, 0).unwrap();
    assert_eq!(added_entry.date, expected_date);

    let entries = provider.load_all_entries().await.unwrap();

    assert_eq!(entries.len(), 3);
    // Find the added entry by title (order depends on UID-based file names).
    let added = entries
        .iter()
        .find(|e| e.title == "Title added")
        .expect("added entry should be present");
    assert_eq!(added.title, String::from("Title added"));
    assert_eq!(added.content, String::from("Content entry added"));
    assert_eq!(added.priority, Some(1));
    assert_eq!(added.date, expected_date);
    assert_eq!(
        added.tags,
        vec![String::from("Tag_1"), String::from("Tag_3")]
    );
}

#[tokio::test]
async fn write_priorities_are_validated() {
    let dir = Builder::new()
        .prefix("vj-priority-write")
        .tempdir()
        .unwrap();
    let mut provider = create_provider(&dir);

    let invalid = provider
        .add_entry(EntryDraft::new(
            Utc::now(),
            String::from("Invalid"),
            Vec::new(),
            Some(42),
        ))
        .await;
    let undefined = provider
        .add_entry(EntryDraft::new(
            Utc::now(),
            String::from("Undefined"),
            Vec::new(),
            Some(0),
        ))
        .await
        .unwrap();

    assert!(invalid.is_err());
    assert_eq!(undefined.priority, None);

    let mut reloaded_provider = create_provider(&dir);
    let reloaded = reloaded_provider.load_all_entries().await.unwrap();
    assert_eq!(reloaded.len(), 1);
    assert_eq!(reloaded[0].title, "Undefined");
    assert_eq!(reloaded[0].priority, None);
}

#[tokio::test]
async fn remove_entry() {
    let dir = Builder::new().prefix("vj-remove").tempdir().unwrap();
    let mut provider = create_provider_with_two_entries(&dir).await;

    let entries = provider.load_all_entries().await.unwrap();
    assert_eq!(entries.len(), 2);
    let id_to_remove = entries.iter().find(|e| e.title == "Title 2").unwrap().id;

    provider.remove_entry(id_to_remove).await.unwrap();

    let entries = provider.load_all_entries().await.unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].title, String::from("Title 1"));
}

#[tokio::test]
async fn restore_entry_validates_priority_and_preserves_id() {
    let dir = Builder::new().prefix("vj-restore").tempdir().unwrap();
    let mut provider = create_provider_with_two_entries(&dir).await;
    let mut restored_entry = provider
        .load_all_entries()
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.title == "Title 1")
        .unwrap();

    provider.remove_entry(restored_entry.id).await.unwrap();
    restored_entry.priority = Some(42);
    assert!(
        provider
            .restore_entry(restored_entry.clone())
            .await
            .is_err()
    );

    restored_entry.priority = Some(9);
    let restored = provider
        .restore_entry(restored_entry.clone())
        .await
        .unwrap();

    assert_eq!(restored, restored_entry);
    let entries = provider.load_all_entries().await.unwrap();
    assert!(entries.contains(&restored_entry));
}

#[tokio::test]
async fn add_after_restore_uses_a_new_id() {
    let dir = Builder::new()
        .prefix("vj-add-after-restore")
        .tempdir()
        .unwrap();
    let mut provider = create_provider(&dir);
    let restored = Entry::from_draft(
        7,
        EntryDraft::new(Utc::now(), String::from("Restored"), Vec::new(), None),
    );

    provider.restore_entry(restored).await.unwrap();
    let added = provider
        .add_entry(EntryDraft::new(
            Utc::now(),
            String::from("Added"),
            Vec::new(),
            None,
        ))
        .await
        .unwrap();

    assert_eq!(added.id, 8);
    let entries = provider.load_all_entries().await.unwrap();
    assert_eq!(entries.len(), 2);
    assert_ne!(entries[0].id, entries[1].id);
}

#[tokio::test]
async fn update_entry() {
    let dir = Builder::new().prefix("vj-update").tempdir().unwrap();
    let mut provider = create_provider_with_two_entries(&dir).await;

    let entries = provider.load_all_entries().await.unwrap();

    // Find entries by title since file ordering is non-deterministic (UUID filenames).
    let mut entry1 = entries
        .iter()
        .find(|e| e.title == "Title 1")
        .cloned()
        .unwrap();
    let mut entry2 = entries
        .iter()
        .find(|e| e.title == "Title 2")
        .cloned()
        .unwrap();

    entry1.content = String::from("Updated Content");
    entry1.tags.pop().unwrap();
    entry1.priority = Some(8);
    entry1.date = Utc.with_ymd_and_hms(2024, 4, 5, 6, 7, 8).unwrap();
    entry2.title = String::from("Updated Title");
    entry2.tags.push(String::from("Tag_4"));
    entry2.priority = None;

    provider.update_entry(entry2).await.unwrap();

    let mut invalid_entry = entry1.clone();
    invalid_entry.priority = Some(12);
    assert!(provider.update_entry(invalid_entry).await.is_err());
    let unchanged = provider
        .load_all_entries()
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.title == "Title 1")
        .unwrap();
    assert_eq!(unchanged.content, "Content entry 1");
    assert_eq!(unchanged.priority, None);

    let updated_entry = provider.update_entry(entry1).await.unwrap();

    let expected_date = Utc.with_ymd_and_hms(2024, 4, 5, 0, 0, 0).unwrap();
    assert_eq!(updated_entry.date, expected_date);
    assert_eq!(updated_entry.priority, Some(8));

    let entries = provider.load_all_entries().await.unwrap();

    assert_eq!(entries.len(), 2);
    let first = entries
        .iter()
        .find(|e| e.content == "Updated Content")
        .expect("updated entry should be present");
    assert_eq!(first.tags.len(), 1);
    assert_eq!(first.priority, Some(8));
    assert_eq!(first.date, expected_date);

    let second = entries
        .iter()
        .find(|e| e.title == "Updated Title")
        .expect("updated entry should be present");
    assert!(second.tags.contains(&String::from("Tag_4")));
    assert_eq!(second.priority, None);
}

#[tokio::test]
async fn escaped_uid_survives_repeated_updates() {
    let dir = Builder::new()
        .prefix("vj-escaped-uid-update")
        .tempdir()
        .unwrap();
    let path = write_calendar(
        &dir,
        "\
BEGIN:VCALENDAR\r
VERSION:2.0\r
BEGIN:VJOURNAL\r
UID:team\\,journal\\\\archive\r
DTSTAMP:20250101T000000Z\r
SUMMARY:Original\r
END:VJOURNAL\r
END:VCALENDAR\r
",
    )
    .await;
    let mut provider = create_provider(&dir);
    let mut entry = provider.load_all_entries().await.unwrap().remove(0);

    entry.title = String::from("First update");
    let mut entry = provider.update_entry(entry).await.unwrap();
    entry.content = String::from("Second update");
    provider.update_entry(entry).await.unwrap();

    let content = tokio::fs::read_to_string(path).await.unwrap();
    let calendar = parse_component(&content).unwrap();
    let journal = &calendar.subcomponents[0];
    let uid = journal.get_only("UID").unwrap();
    assert_eq!(uid.raw_value, r"team\,journal\\archive");
    assert_eq!(uid.value_as_string(), r"team,journal\archive");
    assert_eq!(
        journal.get_only("DESCRIPTION").unwrap().value_as_string(),
        "Second update"
    );
}

#[tokio::test]
async fn export_import() {
    let dir_source = Builder::new().prefix("vj-export-src").tempdir().unwrap();
    let mut provider_source = create_provider_with_two_entries(&dir_source).await;

    let entries = provider_source.load_all_entries().await.unwrap();
    let created_ids: Vec<u32> = entries.iter().map(|e| e.id).collect();

    let dto_source = provider_source
        .get_export_object(&created_ids)
        .await
        .unwrap();

    assert_eq!(dto_source.entries.len(), created_ids.len());

    let dir_dist = Builder::new().prefix("vj-export-dst").tempdir().unwrap();
    let mut provider_dist = create_provider(&dir_dist);

    provider_dist
        .import_entries(dto_source.clone())
        .await
        .unwrap();

    // After import, IDs are freshly assigned; compare by content.
    let imported = provider_dist.load_all_entries().await.unwrap();
    assert_eq!(imported.len(), dto_source.entries.len());

    for draft in &dto_source.entries {
        let found = imported
            .iter()
            .find(|e| e.title == draft.title)
            .unwrap_or_else(|| panic!("expected to find imported entry '{}'", draft.title));
        assert_eq!(found.content, draft.content);
    }
}

#[tokio::test]
async fn assign_priority() {
    let dir = Builder::new().prefix("vj-priority").tempdir().unwrap();
    let mut provider = create_provider_with_two_entries(&dir).await;

    assert!(provider.assign_priority_to_entries(12).await.is_err());
    let entries = provider.load_all_entries().await.unwrap();
    assert_eq!(
        entries
            .iter()
            .find(|entry| entry.title == "Title 1")
            .unwrap()
            .priority,
        None
    );

    provider.assign_priority_to_entries(9).await.unwrap();
    let entries = provider.load_all_entries().await.unwrap();

    let entry_no_prio = entries
        .iter()
        .find(|e| e.title == "Title 1")
        .expect("entry 1 should be present");
    let entry_with_prio = entries
        .iter()
        .find(|e| e.title == "Title 2")
        .expect("entry 2 should be present");

    assert_eq!(entry_no_prio.priority, Some(9));
    // Title 2 already had priority 1, should remain 1.
    assert_eq!(entry_with_prio.priority, Some(1));
}

#[tokio::test]
async fn recurrence_instances_are_hidden_and_master_updates_are_targeted() {
    let dir = Builder::new()
        .prefix("vj-recurrence-update")
        .tempdir()
        .unwrap();
    let path = write_calendar(&dir, RECURRING_CALENDAR).await;
    let mut provider = create_provider(&dir);

    let entries = provider.load_all_entries().await.unwrap();

    assert_eq!(entries.len(), 2);
    assert_ne!(entries[0].id, entries[1].id);
    assert!(
        entries
            .iter()
            .all(|entry| entry.title != "Detached instance")
    );

    let mut master = entries
        .into_iter()
        .find(|entry| entry.title == "Series master")
        .unwrap();
    master.title = String::from("Updated master");
    provider.update_entry(master).await.unwrap();

    let content = tokio::fs::read_to_string(path).await.unwrap();
    let calendar = parse_component(&content).unwrap();
    let recurring: Vec<_> = calendar
        .subcomponents
        .iter()
        .filter(|component| {
            component.name == "VJOURNAL" && component_uid(component) == Some("recurring-journal")
        })
        .collect();

    assert_eq!(recurring.len(), 2);
    let master = recurring
        .iter()
        .find(|component| component.get_only("RECURRENCE-ID").is_none())
        .unwrap();
    assert_eq!(
        master.get_only("SUMMARY").unwrap().value_as_string(),
        "Updated master"
    );
    assert_eq!(
        master.get_only("RRULE").unwrap().raw_value,
        "FREQ=DAILY;COUNT=3"
    );

    let detached = recurring
        .iter()
        .find(|component| component.get_only("RECURRENCE-ID").is_some())
        .unwrap();
    assert_eq!(
        detached.get_only("SUMMARY").unwrap().value_as_string(),
        "Detached instance"
    );
    assert_eq!(
        detached.get_only("DESCRIPTION").unwrap().value_as_string(),
        "Detached content"
    );
}

#[tokio::test]
async fn repeated_recurrence_ids_are_not_treated_as_a_master() {
    let dir = Builder::new()
        .prefix("vj-repeated-recurrence-id")
        .tempdir()
        .unwrap();
    write_calendar(
        &dir,
        "\
BEGIN:VCALENDAR\r
VERSION:2.0\r
BEGIN:VJOURNAL\r
UID:recurring-journal\r
DTSTAMP:20250101T000000Z\r
RECURRENCE-ID;VALUE=DATE:20250102\r
RECURRENCE-ID;VALUE=DATE:20250103\r
SUMMARY:Malformed instance\r
END:VJOURNAL\r
BEGIN:VJOURNAL\r
UID:recurring-journal\r
DTSTAMP:20250101T000000Z\r
SUMMARY:Series master\r
END:VJOURNAL\r
END:VCALENDAR\r
",
    )
    .await;
    let mut provider = create_provider(&dir);

    let entries = provider.load_all_entries().await.unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].title, "Series master");
}

#[tokio::test]
async fn deleting_master_removes_only_its_vjournal_series() {
    let dir = Builder::new()
        .prefix("vj-recurrence-remove")
        .tempdir()
        .unwrap();
    let path = write_calendar(&dir, RECURRING_CALENDAR).await;
    let mut provider = create_provider(&dir);
    let master_id = provider
        .load_all_entries()
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.title == "Series master")
        .unwrap()
        .id;

    provider.remove_entry(master_id).await.unwrap();

    let content = tokio::fs::read_to_string(path).await.unwrap();
    let calendar = parse_component(&content).unwrap();
    assert!(!calendar.subcomponents.iter().any(|component| {
        component.name == "VJOURNAL" && component_uid(component) == Some("recurring-journal")
    }));
    assert!(calendar.subcomponents.iter().any(|component| {
        component.name == "VJOURNAL" && component_uid(component) == Some("standalone-journal")
    }));
    assert!(calendar.subcomponents.iter().any(|component| {
        component.name == "VEVENT" && component_uid(component) == Some("recurring-journal")
    }));
}

#[tokio::test]
async fn escaped_uid_series_can_be_deleted_after_update() {
    let dir = Builder::new()
        .prefix("vj-escaped-uid-remove")
        .tempdir()
        .unwrap();
    let path = write_calendar(
        &dir,
        "\
BEGIN:VCALENDAR\r
VERSION:2.0\r
BEGIN:VJOURNAL\r
UID:series\\,journal\r
DTSTAMP:20250101T000000Z\r
RECURRENCE-ID;VALUE=DATE:20250102\r
SUMMARY:Detached instance\r
END:VJOURNAL\r
BEGIN:VJOURNAL\r
UID:series\\,journal\r
DTSTAMP:20250101T000000Z\r
SUMMARY:Series master\r
END:VJOURNAL\r
BEGIN:VJOURNAL\r
UID:standalone-journal\r
DTSTAMP:20250101T000000Z\r
SUMMARY:Standalone\r
END:VJOURNAL\r
END:VCALENDAR\r
",
    )
    .await;
    let mut provider = create_provider(&dir);
    let mut master = provider
        .load_all_entries()
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.title == "Series master")
        .unwrap();

    master.title = String::from("Updated master");
    let master = provider.update_entry(master).await.unwrap();
    provider.remove_entry(master.id).await.unwrap();

    let content = tokio::fs::read_to_string(path).await.unwrap();
    let calendar = parse_component(&content).unwrap();
    assert_eq!(calendar.subcomponents.len(), 1);
    assert_eq!(
        component_uid(&calendar.subcomponents[0]),
        Some("standalone-journal")
    );
}

#[tokio::test]
async fn duplicate_masters_after_first_are_skipped() {
    let dir = Builder::new()
        .prefix("vj-duplicate-master")
        .tempdir()
        .unwrap();
    write_calendar(
        &dir,
        "\
BEGIN:VCALENDAR\r
VERSION:2.0\r
BEGIN:VJOURNAL\r
UID:duplicate-master\r
DTSTAMP:20250101T000000Z\r
SUMMARY:First master\r
END:VJOURNAL\r
BEGIN:VJOURNAL\r
UID:duplicate-master\r
DTSTAMP:20250102T000000Z\r
SUMMARY:Second master\r
END:VJOURNAL\r
END:VCALENDAR\r
",
    )
    .await;
    let mut provider = create_provider(&dir);

    let entries = provider.load_all_entries().await.unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].title, "First master");
}

#[tokio::test]
async fn empty_directory_loads_no_entries() {
    let dir = Builder::new().prefix("vj-empty").tempdir().unwrap();
    let mut provider = create_provider(&dir);

    let entries = provider.load_all_entries().await.unwrap();

    assert!(entries.is_empty());
}

#[tokio::test]
async fn nonexistent_directory_loads_no_entries() {
    // Use a child of a temp dir that doesn't exist yet.
    let parent = Builder::new().prefix("vj-noexist").tempdir().unwrap();
    let path = parent.path().join("nonexistent");
    let mut provider = VjournalDataProvide::new(path);

    let entries = provider.load_all_entries().await.unwrap();

    assert!(entries.is_empty());
}

#[tokio::test]
async fn roundtrip_preserves_escaped_tag_delimiters() {
    let dir = Builder::new().prefix("vj-tag-escaping").tempdir().unwrap();
    let expected_tags = vec![
        String::from("work,personal"),
        String::from("trailing\\"),
        String::from("plain"),
    ];
    let mut provider = create_provider(&dir);
    provider
        .add_entry(EntryDraft::new(
            Utc::now(),
            String::from("Escaped tags"),
            expected_tags.clone(),
            None,
        ))
        .await
        .unwrap();

    let mut reloaded_provider = create_provider(&dir);
    let entries = reloaded_provider.load_all_entries().await.unwrap();

    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].tags, expected_tags);
}

#[tokio::test]
async fn roundtrip_preserves_content_through_reload() {
    let dir = Builder::new().prefix("vj-roundtrip").tempdir().unwrap();
    let _provider = create_provider_with_two_entries(&dir).await;

    // Load, then create a fresh provider pointing at the same directory to
    // verify persistence on disk.
    let mut provider2 = VjournalDataProvide::new(dir.path().to_path_buf());
    let entries = provider2.load_all_entries().await.unwrap();

    assert_eq!(entries.len(), 2);
    let titles: Vec<&str> = entries.iter().map(|e| e.title.as_str()).collect();
    assert!(titles.contains(&"Title 1"));
    assert!(titles.contains(&"Title 2"));
}

fn protected(entry: Entry) -> Entry {
    entry.with_protection(EncryptedNote {
        version: 1,
        identity: age::x25519::Identity::generate().to_public().to_string(),
        ciphertext:
            "-----BEGIN AGE ENCRYPTED FILE-----\ndGVzdA==\n-----END AGE ENCRYPTED FILE-----\n"
                .into(),
    })
}

#[tokio::test]
async fn deleted_protected_note_restores_only_ciphertext_and_keeps_its_native_uid() {
    let dir = TempDir::new().unwrap();
    let path = write_calendar(&dir, RECURRING_CALENDAR).await;
    let mut provider = create_provider(&dir);
    let entry = provider
        .load_all_entries()
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.title == "Series master")
        .unwrap();
    let locked = protected(entry);
    provider.update_entry(locked.clone()).await.unwrap();
    provider.remove_entry(locked.id).await.unwrap();
    provider.retain_restore_metadata(&[locked.id]);
    assert_eq!(
        provider.restore_entry(locked.clone()).await.unwrap(),
        locked
    );
    let text = std::fs::read_to_string(path).unwrap();
    for private in ["Master content", "Detached content", "RRULE:"] {
        assert!(!text.contains(private));
    }
    let calendar = parse_component(&text).unwrap();
    let restored = calendar
        .subcomponents
        .iter()
        .find(|component| component.name == "X-TUI-JOURNAL-LOCKED")
        .unwrap();
    assert_eq!(component_uid(restored), Some("recurring-journal"));
    assert!(provider.load_all_entries().await.unwrap().contains(&locked));
}

#[tokio::test]
async fn discarded_restore_metadata_cannot_silently_recreate_an_incomplete_calendar() {
    let dir = TempDir::new().unwrap();
    let mut provider = create_provider(&dir);
    let entry = provider
        .add_entry(EntryDraft::new(Utc::now(), "deleted".into(), vec![], None))
        .await
        .unwrap();
    provider.remove_entry(entry.id).await.unwrap();
    provider.retain_restore_metadata(&[]);
    assert!(provider.restore_entry(entry.clone()).await.is_err());
    let added = provider
        .add_entry(EntryDraft::new(Utc::now(), "new".into(), vec![], None))
        .await
        .unwrap();
    assert!(added.id > entry.id);
    assert_eq!(provider.load_all_entries().await.unwrap(), vec![added]);
}

#[tokio::test]
async fn protection_encrypts_whole_recurrence_family_and_restores_unknown_metadata() {
    let dir = TempDir::new().unwrap();
    let native = RECURRING_CALENDAR.replace(
        "DESCRIPTION:Master content",
        "DESCRIPTION:Master content\r\nX-PRIVATE;LANGUAGE=en:Sensitive metadata",
    );
    let path = write_calendar(&dir, &native).await;
    let mut provider = create_provider(&dir);
    let entries = provider.load_all_entries().await.unwrap();
    let master = entries
        .iter()
        .find(|entry| entry.title == "Series master")
        .unwrap()
        .clone();
    let mut sibling = entries
        .iter()
        .find(|entry| entry.title == "Standalone")
        .unwrap()
        .clone();
    let metadata = provider
        .protection_metadata(master.id)
        .await
        .unwrap()
        .unwrap();
    assert!(metadata.contains("Detached content"));
    assert!(metadata.contains("Sensitive metadata"));
    let locked = protected(master.clone());
    provider.update_entry(locked.clone()).await.unwrap();
    let saved = tokio::fs::read_to_string(&path).await.unwrap();
    for private in [
        "Detached content",
        "Detached instance",
        "Master content",
        "Sensitive metadata",
        "RRULE:",
    ] {
        assert!(!saved.contains(private), "{private}");
    }
    assert!(saved.contains("BEGIN:X-TUI-JOURNAL-LOCKED"));
    assert!(saved.contains("Event with matching UID"));
    assert_eq!(
        provider
            .load_all_entries()
            .await
            .unwrap()
            .iter()
            .find(|entry| entry.id == master.id),
        Some(&locked)
    );
    sibling.content = "Edited sibling".into();
    provider.update_entry(sibling.clone()).await.unwrap();
    assert!(provider.load_all_entries().await.unwrap().contains(&locked));
    provider
        .update_entry_with_metadata(master.clone(), Some(&metadata))
        .await
        .unwrap();
    let restored = parse_component(&tokio::fs::read_to_string(&path).await.unwrap()).unwrap();
    let family: Vec<_> = restored
        .subcomponents
        .iter()
        .filter(|component| {
            component.name == "VJOURNAL" && component_uid(component) == Some("recurring-journal")
        })
        .collect();
    assert_eq!(family.len(), 2);
    assert!(family.iter().any(|c| {
        c.get_only("X-PRIVATE").is_some_and(|p| {
            p.value_as_string() == "Sensitive metadata"
                && p.params.get("LANGUAGE").is_some_and(|value| value == "en")
        })
    }));
    assert!(family.iter().any(|c| {
        c.get_only("DESCRIPTION")
            .is_some_and(|p| p.value_as_string() == "Detached content")
    }));
    let entries = provider.load_all_entries().await.unwrap();
    assert!(entries.contains(&master));
    assert!(entries.contains(&sibling));
}

#[tokio::test]
async fn protecting_cross_file_recurrence_families_fails_without_writing() {
    let dir = TempDir::new().unwrap();
    let path = write_calendar(&dir, RECURRING_CALENDAR).await;
    let extra = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VJOURNAL\r\nUID:recurring-journal\r\nRECURRENCE-ID;VALUE=DATE:20250110\r\nDESCRIPTION:Separate private instance\r\nEND:VJOURNAL\r\nEND:VCALENDAR\r\n";
    tokio::fs::write(dir.path().join("other.ics"), extra)
        .await
        .unwrap();
    let mut provider = create_provider(&dir);
    let master = provider
        .load_all_entries()
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.title == "Series master")
        .unwrap();
    assert!(provider.protection_metadata(master.id).await.is_err());
    assert!(provider.update_entry(protected(master)).await.is_err());
    assert_eq!(
        tokio::fs::read_to_string(path).await.unwrap(),
        RECURRING_CALENDAR
    );
}

#[tokio::test]
async fn external_calendar_changes_block_update_removal_and_protection() {
    let dir = TempDir::new().unwrap();
    let path = write_calendar(&dir, RECURRING_CALENDAR).await;
    let mut provider = create_provider(&dir);
    let master = provider
        .load_all_entries()
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.title == "Series master")
        .unwrap();
    let changed = RECURRING_CALENDAR.replace("Master content", "Externally edited content");
    tokio::fs::write(&path, &changed).await.unwrap();
    assert!(provider.update_entry(master.clone()).await.is_err());
    assert!(provider.remove_entry(master.id).await.is_err());
    assert!(provider.protection_metadata(master.id).await.is_err());
    assert_eq!(tokio::fs::read_to_string(path).await.unwrap(), changed);
}

#[tokio::test]
async fn protected_calendar_import_preserves_timestamp_and_excludes_priority_assignment() {
    let dir = TempDir::new().unwrap();
    let mut provider = create_provider(&dir);
    let original = protected(Entry::from_draft(
        0,
        EntryDraft::new(
            Utc.with_ymd_and_hms(2025, 1, 2, 3, 4, 5).unwrap(),
            "Visible title".into(),
            vec![],
            None,
        ),
    ));
    assert_eq!(
        provider
            .add_entry(EntryDraft::from_entry(original.clone()))
            .await
            .unwrap(),
        original
    );
    provider.assign_priority_to_entries(8).await.unwrap();
    assert_eq!(
        provider.load_all_entries().await.unwrap(),
        vec![original.clone()]
    );
    assert_eq!(
        provider
            .get_export_object(&[original.id])
            .await
            .unwrap()
            .entries,
        vec![EntryDraft::from_entry(original.clone())]
    );
    provider.remove_entry(original.id).await.unwrap();
    assert!(provider.load_all_entries().await.unwrap().is_empty());
    assert_eq!(
        provider.restore_entry(original.clone()).await.unwrap(),
        original
    );
}

#[tokio::test]
async fn invalid_protected_calendar_is_reported_instead_of_hidden() {
    let dir = TempDir::new().unwrap();
    write_calendar(&dir, "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:X-TUI-JOURNAL-LOCKED\r\nUID:locked\r\nSUMMARY:Visible\r\nEND:X-TUI-JOURNAL-LOCKED\r\nEND:VCALENDAR\r\n").await;
    assert!(create_provider(&dir).load_all_entries().await.is_err());
}

#[cfg(unix)]
#[tokio::test]
async fn atomic_calendar_updates_preserve_permissions_and_leave_no_plaintext_temporary_files() {
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new().unwrap();
    let path = write_calendar(&dir, RECURRING_CALENDAR).await;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
    let mut provider = create_provider(&dir);
    let master = provider
        .load_all_entries()
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.title == "Series master")
        .unwrap();
    provider.update_entry(protected(master)).await.unwrap();
    assert_eq!(
        std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o640
    );
    let filenames: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    assert_eq!(filenames.len(), 2);
    assert!(filenames.contains(&".tui-journal.lock".into()));
}

#[tokio::test]
async fn protected_recurrence_conflict_is_reported_without_overwriting_private_instance() {
    let dir = TempDir::new().unwrap();
    let path = write_calendar(&dir, RECURRING_CALENDAR).await;
    let mut provider = create_provider(&dir);
    let master = provider
        .load_all_entries()
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.title == "Series master")
        .unwrap();
    provider.update_entry(protected(master)).await.unwrap();
    let extra = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VJOURNAL\r\nUID:recurring-journal\r\nRECURRENCE-ID;VALUE=DATE:20250110\r\nDESCRIPTION:Unencrypted private instance\r\nEND:VJOURNAL\r\nEND:VCALENDAR\r\n";
    tokio::fs::write(dir.path().join("other.ics"), extra)
        .await
        .unwrap();
    let before = tokio::fs::read(&path).await.unwrap();
    assert!(provider.load_all_entries().await.is_err());
    assert_eq!(tokio::fs::read(path).await.unwrap(), before);
    assert_eq!(
        tokio::fs::read_to_string(dir.path().join("other.ics"))
            .await
            .unwrap(),
        extra
    );
}

#[tokio::test]
async fn export_does_not_accept_external_changes_as_a_new_write_baseline() {
    let dir = TempDir::new().unwrap();
    let path = write_calendar(&dir, RECURRING_CALENDAR).await;
    let mut provider = create_provider(&dir);
    let master = provider
        .load_all_entries()
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.title == "Series master")
        .unwrap();
    let changed = RECURRING_CALENDAR.replace("Master content", "External content");
    tokio::fs::write(&path, &changed).await.unwrap();
    assert!(provider.get_export_object(&[master.id]).await.is_err());
    assert!(provider.update_entry(master).await.is_err());
    assert_eq!(tokio::fs::read_to_string(path).await.unwrap(), changed);
}

#[tokio::test]
async fn protection_includes_case_insensitive_recurrence_components_and_properties() {
    let dir = TempDir::new().unwrap();
    let calendar = RECURRING_CALENDAR
        .replacen("BEGIN:VJOURNAL", "BEGIN:vjournal", 1)
        .replacen("END:VJOURNAL", "END:vjournal", 1)
        .replacen("UID:recurring-journal", "uid:recurring-journal", 1)
        .replacen("RECURRENCE-ID;", "recurrence-id;", 1);
    let path = write_calendar(&dir, &calendar).await;
    let mut provider = create_provider(&dir);
    let master = provider
        .load_all_entries()
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.title == "Series master")
        .unwrap();
    let metadata = provider
        .protection_metadata(master.id)
        .await
        .unwrap()
        .unwrap();
    assert!(metadata.contains("Detached content"));
    provider.update_entry(protected(master)).await.unwrap();
    assert!(
        !tokio::fs::read_to_string(path)
            .await
            .unwrap()
            .contains("Detached content")
    );
}

#[tokio::test]
async fn ambiguous_family_uid_prevents_partial_encryption() {
    let dir = TempDir::new().unwrap();
    let calendar = RECURRING_CALENDAR.replacen(
        "UID:recurring-journal",
        "UID:recurring-journal\r\nuid:another-journal",
        1,
    );
    let path = write_calendar(&dir, &calendar).await;
    let mut provider = create_provider(&dir);
    let master = provider
        .load_all_entries()
        .await
        .unwrap()
        .into_iter()
        .find(|entry| entry.title == "Series master")
        .unwrap();
    assert!(provider.protection_metadata(master.id).await.is_err());
    assert!(provider.update_entry(protected(master)).await.is_err());
    assert_eq!(tokio::fs::read_to_string(path).await.unwrap(), calendar);
}

#[tokio::test]
async fn removing_imported_protection_preserves_literal_backslashes_and_line_breaks() {
    let dir = TempDir::new().unwrap();
    let mut provider = create_provider(&dir);
    let text = "Literal \\n and \\N; comma, and CRLF\r\n";
    let original = Entry::from_draft(
        0,
        EntryDraft::new(
            Utc.with_ymd_and_hms(2025, 1, 2, 0, 0, 0).unwrap(),
            text.into(),
            vec![r"tag\n".into(), r"tag\N".into()],
            Some(2),
        )
        .with_content(text.into()),
    );
    provider
        .add_entry(EntryDraft::from_entry(protected(original.clone())))
        .await
        .unwrap();
    provider
        .update_entry_with_metadata(
            original.clone(),
            Some(r#"{"format":"file-v1","document":"file metadata"}"#),
        )
        .await
        .unwrap();
    assert_eq!(provider.load_all_entries().await.unwrap(), vec![original]);
}

#[tokio::test]
async fn removing_protection_preserves_unchanged_native_properties_and_applies_edits() {
    let native = "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:VJOURNAL\r\nUID:timed-journal\r\nDTSTAMP:20250101T000000Z\r\nDTSTART;TZID=Europe/Berlin:20250102T090000\r\nSUMMARY;LANGUAGE=en:Title\r\nDESCRIPTION;LANGUAGE=en:Body\r\nDESCRIPTION;LANGUAGE=de:Zweiter Absatz\r\nCATEGORIES;LANGUAGE=en:one,two\r\nPRIORITY:0\r\nRRULE:FREQ=DAILY;COUNT=3\r\nEND:VJOURNAL\r\nEND:VCALENDAR\r\n";
    let before = parse_component(native).unwrap().subcomponents.remove(0);
    for edit in [false, true] {
        let dir = TempDir::new().unwrap();
        let path = write_calendar(&dir, native).await;
        let mut provider = create_provider(&dir);
        let mut entry = provider.load_all_entries().await.unwrap().remove(0);
        let metadata = provider
            .protection_metadata(entry.id)
            .await
            .unwrap()
            .unwrap();
        provider
            .update_entry(protected(entry.clone()))
            .await
            .unwrap();
        if edit {
            entry.content = "Edited body".into();
        }
        provider
            .update_entry_with_metadata(entry.clone(), Some(&metadata))
            .await
            .unwrap();
        let restored = parse_component(&tokio::fs::read_to_string(path).await.unwrap())
            .unwrap()
            .subcomponents
            .remove(0);
        for property in [
            "SUMMARY",
            "DTSTART",
            "CATEGORIES",
            "PRIORITY",
            "RRULE",
            "DESCRIPTION",
        ] {
            if edit && property == "DESCRIPTION" {
                continue;
            }
            let values = |component: &Component| {
                component
                    .get_all(property)
                    .iter()
                    .map(|value| {
                        (
                            value.raw_value.clone(),
                            value.params.clone(),
                            value.prop_group.clone(),
                        )
                    })
                    .collect::<Vec<_>>()
            };
            assert_eq!(values(&restored), values(&before), "{property}");
        }
        assert_eq!(provider.load_all_entries().await.unwrap(), vec![entry]);
    }
}

#[tokio::test]
async fn invalid_locked_component_identity_is_reported_instead_of_hidden() {
    for properties in [
        "SUMMARY:Visible",
        "UID:\r\nSUMMARY:Visible",
        "UID:one\r\nUID:two\r\nSUMMARY:Visible",
        "UID:one\r\nRECURRENCE-ID;VALUE=DATE:20250102\r\nSUMMARY:Visible",
    ] {
        let dir = TempDir::new().unwrap();
        write_calendar(
            &dir,
            &format!("BEGIN:VCALENDAR\r\nVERSION:2.0\r\nBEGIN:X-TUI-JOURNAL-LOCKED\r\n{properties}\r\nEND:X-TUI-JOURNAL-LOCKED\r\nEND:VCALENDAR\r\n"),
        )
        .await;
        assert!(
            create_provider(&dir).load_all_entries().await.is_err(),
            "{properties}"
        );
    }
}

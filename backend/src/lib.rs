use chrono::{DateTime, Utc};

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

mod persistence;
pub mod protection;
pub use protection::EncryptedNote;

#[cfg(feature = "file")]
mod file;
#[cfg(feature = "file")]
pub use file::{FileDataProvider, FileFormat};

#[cfg(feature = "json")]
mod json;
#[cfg(feature = "json")]
pub use json::JsonDataProvide;

#[cfg(feature = "sqlite")]
mod sqlite;
#[cfg(feature = "sqlite")]
pub use sqlite::SqliteDataProvide;

#[cfg(feature = "vjournal")]
mod vjournal;
#[cfg(feature = "vjournal")]
pub use vjournal::VjournalDataProvide;

pub const TRANSFER_DATA_VERSION: u16 = 101;
pub const LEGACY_TRANSFER_DATA_VERSION: u16 = 100;

#[derive(Debug, thiserror::Error)]
pub enum ModifyEntryError {
    #[error("{0}")]
    ValidationError(String),
    #[error("{0}")]
    DataError(#[from] anyhow::Error),
}

// The warning can be suppressed since this will be used with the code base of this app only
#[allow(async_fn_in_trait)]
pub trait DataProvider {
    /// Retain backend-specific deletion snapshots only while undo/redo can restore them.
    fn retain_restore_metadata(&mut self, _entry_ids: &[u32]) {}

    async fn load_all_entries(&mut self) -> anyhow::Result<Vec<Entry>>;
    async fn add_entry(&mut self, entry: EntryDraft) -> Result<Entry, ModifyEntryError>;
    /// Restores an entry with its existing id. Implementations must not overwrite another entry.
    async fn restore_entry(&mut self, entry: Entry) -> Result<Entry, ModifyEntryError>;
    async fn remove_entry(&mut self, entry_id: u32) -> anyhow::Result<()>;
    async fn update_entry(&mut self, entry: Entry) -> Result<Entry, ModifyEntryError>;
    /// Native metadata is encrypted along with the portable note fields.
    async fn protection_metadata(&mut self, _entry_id: u32) -> anyhow::Result<Option<String>> {
        Ok(None)
    }
    async fn update_entry_with_metadata(
        &mut self,
        entry: Entry,
        _metadata: Option<&str>,
    ) -> Result<Entry, ModifyEntryError> {
        self.update_entry(entry).await
    }
    async fn get_export_object(&mut self, entries_ids: &[u32]) -> anyhow::Result<EntriesDTO>;
    async fn import_entries(&mut self, entries_dto: EntriesDTO) -> anyhow::Result<()> {
        entries_dto.validate()?;

        for entry_draft in entries_dto.entries {
            self.add_entry(entry_draft).await?;
        }

        Ok(())
    }
    /// Assigns priority to all entries that don't have a priority assigned to
    async fn assign_priority_to_entries(&mut self, priority: u32) -> anyhow::Result<()>;
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub id: u32,
    pub date: DateTime<Utc>,
    pub title: String,
    pub content: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub priority: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protection: Option<EncryptedNote>,
}

impl Entry {
    #[allow(dead_code)]
    pub fn new(
        id: u32,
        date: DateTime<Utc>,
        title: String,
        content: String,
        tags: Vec<String>,
        priority: Option<u32>,
    ) -> Self {
        Self {
            id,
            date,
            title,
            content,
            tags,
            priority,
            protection: None,
        }
    }

    pub fn from_draft(id: u32, draft: EntryDraft) -> Self {
        Self {
            id,
            date: draft.date,
            title: draft.title,
            content: draft.content,
            tags: draft.tags,
            priority: draft.priority,
            protection: draft.protection,
        }
    }

    pub fn validate_protection(&self) -> anyhow::Result<()> {
        protection::validate_protected_entry(self).map_err(Into::into)
    }

    /// Only the public fields accompany an encrypted payload across the storage boundary.
    pub fn with_protection(mut self, protection: EncryptedNote) -> Self {
        self.content.zeroize();
        self.tags.zeroize();
        self.priority.zeroize();
        self.priority = None;
        self.protection = Some(protection);
        self
    }
}

impl std::fmt::Debug for Entry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Entry")
            .field("id", &self.id)
            .field("protected", &self.protection.is_some())
            .finish_non_exhaustive()
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntryDraft {
    pub date: DateTime<Utc>,
    pub title: String,
    pub content: String,
    pub tags: Vec<String>,
    pub priority: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protection: Option<EncryptedNote>,
}

impl EntryDraft {
    pub fn new(
        date: DateTime<Utc>,
        title: String,
        tags: Vec<String>,
        priority: Option<u32>,
    ) -> Self {
        let content = String::new();
        Self {
            date,
            title,
            content,
            tags,
            priority,
            protection: None,
        }
    }

    #[must_use]
    pub fn with_content(mut self, content: String) -> Self {
        self.content.zeroize();
        self.content = content;
        self
    }

    pub fn from_entry(entry: Entry) -> Self {
        Self {
            date: entry.date,
            title: entry.title,
            content: entry.content,
            tags: entry.tags,
            priority: entry.priority,
            protection: entry.protection,
        }
    }
}

impl std::fmt::Debug for EntryDraft {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EntryDraft")
            .field("protected", &self.protection.is_some())
            .finish_non_exhaustive()
    }
}

/// Entries data transfer object. Protected records use a shape that legacy importers reject.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntriesDTO {
    pub version: u16,
    pub entries: Vec<EntryDraft>,
}

impl EntriesDTO {
    pub fn new(entries: Vec<EntryDraft>) -> Self {
        Self {
            version: TRANSFER_DATA_VERSION,
            entries,
        }
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        anyhow::ensure!(
            matches!(
                self.version,
                LEGACY_TRANSFER_DATA_VERSION | TRANSFER_DATA_VERSION
            ),
            "Unsupported journal transfer version {}",
            self.version
        );
        for draft in &self.entries {
            anyhow::ensure!(
                self.version != LEGACY_TRANSFER_DATA_VERSION || draft.protection.is_none(),
                "Encrypted records require the current journal transfer format"
            );
            anyhow::ensure!(!draft.title.is_empty(), "Entry title can't be empty");
            if let Some(protection) = &draft.protection {
                protection.validate()?;
                anyhow::ensure!(
                    draft.content.is_empty() && draft.tags.is_empty() && draft.priority.is_none(),
                    protection::ProtectionError::PlaintextFields
                );
            }
        }
        Ok(())
    }
}

impl Serialize for EntriesDTO {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.validate().map_err(serde::ser::Error::custom)?;
        let entries = self
            .entries
            .iter()
            .map(|entry| match &entry.protection {
                Some(protection) => transfer_format::EntryRef::Protected {
                    protected: transfer_format::ProtectedRef {
                        date: entry.date,
                        title: &entry.title,
                        protection,
                    },
                },
                None => transfer_format::EntryRef::Plain(entry),
            })
            .collect();
        transfer_format::DocumentRef {
            version: self.version,
            entries,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for EntriesDTO {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let document = transfer_format::Document::deserialize(deserializer)?;
        let entries = document
            .entries
            .into_iter()
            .map(|entry| match entry {
                transfer_format::TransferEntry::Protected { protected } => EntryDraft {
                    date: protected.date,
                    title: protected.title,
                    content: String::new(),
                    tags: Vec::new(),
                    priority: None,
                    protection: Some(protected.protection),
                },
                transfer_format::TransferEntry::Plain(plain) => EntryDraft {
                    date: plain.date,
                    title: plain.title,
                    content: plain.content,
                    tags: plain.tags,
                    priority: plain.priority,
                    protection: None,
                },
            })
            .collect();
        let document = Self {
            version: document.version,
            entries,
        };
        document.validate().map_err(serde::de::Error::custom)?;
        Ok(document)
    }
}

mod transfer_format {
    use super::*;

    #[derive(Serialize)]
    pub(super) struct DocumentRef<'a> {
        pub version: u16,
        pub entries: Vec<EntryRef<'a>>,
    }

    #[derive(Serialize)]
    #[serde(untagged)]
    pub(super) enum EntryRef<'a> {
        Protected { protected: ProtectedRef<'a> },
        Plain(&'a EntryDraft),
    }

    #[derive(Serialize)]
    pub(super) struct ProtectedRef<'a> {
        pub date: DateTime<Utc>,
        pub title: &'a str,
        pub protection: &'a EncryptedNote,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct Document {
        pub version: u16,
        pub entries: Vec<TransferEntry>,
    }

    #[derive(Deserialize)]
    #[serde(untagged, deny_unknown_fields)]
    pub(super) enum TransferEntry {
        Protected { protected: Protected },
        Plain(Plain),
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct Protected {
        pub date: DateTime<Utc>,
        pub title: String,
        pub protection: EncryptedNote,
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct Plain {
        pub date: DateTime<Utc>,
        pub title: String,
        pub content: String,
        pub tags: Vec<String>,
        pub priority: Option<u32>,
    }
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;

    use super::*;

    fn sample_draft() -> EntryDraft {
        EntryDraft {
            date: Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap(),
            title: String::from("Draft"),
            content: String::from("Body"),
            tags: vec![String::from("one"), String::from("two")],
            priority: Some(3),
            protection: None,
        }
    }

    struct ImportStubProvider {
        added_entries: Vec<EntryDraft>,
        fail_on_call: Option<usize>,
    }

    impl ImportStubProvider {
        fn new(fail_on_call: Option<usize>) -> Self {
            Self {
                added_entries: Vec::new(),
                fail_on_call,
            }
        }
    }

    impl DataProvider for ImportStubProvider {
        async fn load_all_entries(&mut self) -> anyhow::Result<Vec<Entry>> {
            unreachable!("not used in these tests");
        }

        async fn add_entry(&mut self, entry: EntryDraft) -> Result<Entry, ModifyEntryError> {
            let call_idx = self.added_entries.len();
            self.added_entries.push(entry.clone());

            if self.fail_on_call == Some(call_idx) {
                return Err(ModifyEntryError::ValidationError(format!(
                    "fail on {call_idx}"
                )));
            }

            Ok(Entry::from_draft(call_idx as u32, entry))
        }

        async fn restore_entry(&mut self, _entry: Entry) -> Result<Entry, ModifyEntryError> {
            unreachable!("not used in these tests");
        }

        async fn remove_entry(&mut self, _entry_id: u32) -> anyhow::Result<()> {
            unreachable!("not used in these tests");
        }

        async fn update_entry(&mut self, _entry: Entry) -> Result<Entry, ModifyEntryError> {
            unreachable!("not used in these tests");
        }

        async fn get_export_object(&mut self, _entries_ids: &[u32]) -> anyhow::Result<EntriesDTO> {
            unreachable!("not used in these tests");
        }

        async fn assign_priority_to_entries(&mut self, _priority: u32) -> anyhow::Result<()> {
            unreachable!("not used in these tests");
        }
    }

    #[test]
    fn draft_to_entry() {
        let draft = sample_draft();

        let entry = Entry::from_draft(7, draft.clone());

        assert_eq!(entry.id, 7);
        assert_eq!(entry.date, draft.date);
        assert_eq!(entry.title, draft.title);
        assert_eq!(entry.content, draft.content);
        assert_eq!(entry.tags, draft.tags);
        assert_eq!(entry.priority, draft.priority);
    }

    #[test]
    fn with_content_replaces_only_body() {
        let draft = sample_draft();

        let updated = draft.clone().with_content(String::from("Updated"));

        assert_eq!(updated.content, "Updated");
        assert_eq!(updated.date, draft.date);
        assert_eq!(updated.title, draft.title);
        assert_eq!(updated.tags, draft.tags);
        assert_eq!(updated.priority, draft.priority);
    }

    #[test]
    fn from_entry_drops_id_only() {
        let entry = Entry::new(
            11,
            Utc.with_ymd_and_hms(2023, 11, 12, 13, 14, 15).unwrap(),
            String::from("Title"),
            String::from("Content"),
            vec![String::from("tag")],
            Some(2),
        );

        let draft = EntryDraft::from_entry(entry.clone());

        assert_eq!(draft.date, entry.date);
        assert_eq!(draft.title, entry.title);
        assert_eq!(draft.content, entry.content);
        assert_eq!(draft.tags, entry.tags);
        assert_eq!(draft.priority, entry.priority);
    }

    #[test]
    fn dto_sets_version() {
        let dto = EntriesDTO::new(vec![sample_draft()]);

        assert_eq!(dto.version, TRANSFER_DATA_VERSION);
        assert_eq!(dto.entries, vec![sample_draft()]);
    }

    #[tokio::test]
    async fn import_entries_keeps_order() {
        let mut provider = ImportStubProvider::new(None);
        let entries = vec![
            sample_draft(),
            EntryDraft::new(
                Utc.with_ymd_and_hms(2025, 6, 7, 8, 9, 10).unwrap(),
                String::from("Second"),
                vec![String::from("x")],
                None,
            ),
        ];

        provider
            .import_entries(EntriesDTO::new(entries.clone()))
            .await
            .unwrap();

        assert_eq!(provider.added_entries, entries);
    }

    #[tokio::test]
    async fn import_entries_stops_on_error() {
        let mut provider = ImportStubProvider::new(Some(1));
        let entries = vec![
            sample_draft(),
            EntryDraft::new(
                Utc.with_ymd_and_hms(2025, 1, 1, 0, 0, 0).unwrap(),
                String::from("Second"),
                vec![],
                None,
            ),
            EntryDraft::new(
                Utc.with_ymd_and_hms(2025, 1, 2, 0, 0, 0).unwrap(),
                String::from("Third"),
                vec![],
                None,
            ),
        ];

        let err = provider
            .import_entries(EntriesDTO::new(entries.clone()))
            .await
            .unwrap_err();

        assert_eq!(err.to_string(), "fail on 1");

        // The stub records the draft before failing, so the third entry proves import stopped.
        assert_eq!(provider.added_entries, entries[..2]);
    }

    fn protected_draft() -> EntryDraft {
        static DRAFT: std::sync::LazyLock<EntryDraft> = std::sync::LazyLock::new(|| {
            let entry = Entry::new(
                42,
                Utc.with_ymd_and_hms(2026, 10, 24, 1, 2, 3).unwrap(),
                "Public protected title".into(),
                "PRIVATE_TRANSFER_BODY".into(),
                vec!["PRIVATE_TRANSFER_TAG".into()],
                Some(7),
            );
            let protection = protection::protect(
                &entry,
                &protection::SecretString::from("transfer test password".to_owned()),
                None,
            )
            .unwrap();
            EntryDraft::from_entry(entry.with_protection(protection))
        });
        DRAFT.clone()
    }

    #[test]
    fn protected_transfer_rejects_legacy_reader_and_preserves_encryption() {
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct LegacyDraft {
            date: DateTime<Utc>,
            title: String,
            content: String,
            tags: Vec<String>,
            priority: Option<u32>,
        }
        #[derive(Deserialize)]
        #[allow(dead_code)]
        struct LegacyDocument {
            version: u16,
            entries: Vec<LegacyDraft>,
        }
        let document = EntriesDTO::new(vec![sample_draft(), protected_draft()]);
        let json = serde_json::to_string(&document).unwrap();
        assert!(serde_json::from_str::<LegacyDocument>(&json).is_err());
        assert!(!json.contains("PRIVATE_TRANSFER_BODY"));
        assert!(!json.contains("PRIVATE_TRANSFER_TAG"));
        assert!(json.contains("Public protected title"));
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert!(value["entries"][1].get("content").is_none());
        assert!(value["entries"][1].get("title").is_none());
        assert!(value["entries"][1].get("protected").is_some());
        let decoded: EntriesDTO = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded, document);
        let entry = Entry::from_draft(99, decoded.entries[1].clone());
        let unlocked = protection::unlock(
            &entry,
            &protection::SecretString::from("transfer test password".to_owned()),
        )
        .unwrap();
        assert_eq!(unlocked.content, "PRIVATE_TRANSFER_BODY");
        assert_eq!(unlocked.tags, ["PRIVATE_TRANSFER_TAG"]);
        assert_eq!(unlocked.priority, Some(7));
    }

    #[test]
    fn plaintext_transfer_accepts_legacy_and_current_versions() {
        for version in [LEGACY_TRANSFER_DATA_VERSION, TRANSFER_DATA_VERSION] {
            let json = serde_json::json!({ "version": version, "entries": [sample_draft()] });
            let decoded: EntriesDTO = serde_json::from_value(json.clone()).unwrap();
            assert_eq!(decoded.version, version);
            assert_eq!(decoded.entries, vec![sample_draft()]);
            assert_eq!(serde_json::to_value(decoded).unwrap(), json);
        }
    }

    #[test]
    fn transfer_rejects_unknown_versions_and_downgraded_encryption() {
        let protected = EntriesDTO::new(vec![protected_draft()]);
        let mut value = serde_json::to_value(&protected).unwrap();
        value["version"] = LEGACY_TRANSFER_DATA_VERSION.into();
        assert!(serde_json::from_value::<EntriesDTO>(value).is_err());
        for version in [
            0,
            LEGACY_TRANSFER_DATA_VERSION - 1,
            TRANSFER_DATA_VERSION + 1,
            u16::MAX,
        ] {
            let invalid = EntriesDTO {
                version,
                entries: vec![sample_draft()],
            };
            assert!(serde_json::to_string(&invalid).is_err());
            assert!(
                serde_json::from_value::<EntriesDTO>(serde_json::json!({
                    "version": version, "entries": [sample_draft()],
                }))
                .is_err()
            );
        }
    }

    #[test]
    fn transfer_rejects_flat_or_mixed_protected_records() {
        let protected = protected_draft();
        let flat = serde_json::to_value(&protected).unwrap();
        assert!(
            serde_json::from_value::<EntriesDTO>(serde_json::json!({
                "version": TRANSFER_DATA_VERSION, "entries": [flat],
            }))
            .is_err()
        );
        let original = serde_json::to_value(EntriesDTO::new(vec![protected])).unwrap();
        for location in ["outside", "inside"] {
            let mut mixed = original.clone();
            let entry = &mut mixed["entries"][0];
            let fields = if location == "inside" {
                &mut entry["protected"]
            } else {
                entry
            };
            fields["content"] = "PRIVATE_MIXED_CONTENT".into();
            assert!(serde_json::from_value::<EntriesDTO>(mixed).is_err());
        }
        let mut malformed = original;
        malformed["entries"][0]["protected"]["protection"]["version"] = 999.into();
        assert!(serde_json::from_value::<EntriesDTO>(malformed).is_err());
    }

    #[tokio::test]
    async fn invalid_transfer_is_rejected_before_any_import_write() {
        let mut invalid_fields = protected_draft();
        invalid_fields.content = "PRIVATE_MIXED_CONTENT".into();
        let mut invalid_title = sample_draft();
        invalid_title.title.clear();
        let mut invalid_envelope = protected_draft();
        invalid_envelope.protection.as_mut().unwrap().version = 999;
        let documents = [
            EntriesDTO::new(vec![sample_draft(), invalid_fields]),
            EntriesDTO::new(vec![sample_draft(), invalid_title]),
            EntriesDTO::new(vec![sample_draft(), invalid_envelope]),
            EntriesDTO {
                version: LEGACY_TRANSFER_DATA_VERSION,
                entries: vec![sample_draft(), protected_draft()],
            },
            EntriesDTO {
                version: TRANSFER_DATA_VERSION + 1,
                entries: vec![sample_draft()],
            },
        ];
        for document in documents {
            let mut provider = ImportStubProvider::new(None);
            assert!(serde_json::to_string(&document).is_err());
            let error = provider.import_entries(document).await.unwrap_err();
            assert!(!error.to_string().contains("PRIVATE_MIXED_CONTENT"));
            assert!(provider.added_entries.is_empty());
        }
    }
}

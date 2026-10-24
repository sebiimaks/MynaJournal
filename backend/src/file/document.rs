use anyhow::{Context, Result, bail, ensure};
use chrono::{DateTime, FixedOffset, SecondsFormat, Utc};
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::{EncryptedNote, Entry, EntryDraft};

const MAX_HEADER_BYTES: usize = 1024 * 1024;

/// Metadata and body stay separate so body-only saves preserve the original header.
#[derive(Clone)]
pub(super) struct JournalDocument {
    pub uid: Uuid,
    pub created: DateTime<FixedOffset>,
    metadata: Value,
    body: String,
    original_header: Option<String>,
}

#[derive(Deserialize)]
struct Metadata {
    title: String,
    date: DateTime<Utc>,
    created: DateTime<FixedOffset>,
    #[serde(default)]
    tags: Vec<String>,
    tui_journal: JournalMetadata,
}

#[derive(Deserialize)]
struct JournalMetadata {
    version: u32,
    id: String,
    #[serde(default)]
    priority: Option<u32>,
    #[serde(default)]
    protection: Option<ProtectionMetadata>,
}

#[derive(Deserialize)]
struct ProtectionMetadata {
    version: u32,
    identity: String,
}

impl JournalDocument {
    pub fn new(draft: &EntryDraft, created: DateTime<FixedOffset>) -> Result<Self> {
        Entry::from_draft(0, draft.clone()).validate_protection()?;
        let uid = Uuid::new_v4();
        let mut metadata = json!({
            "title": draft.title,
            "date": draft.date.to_rfc3339_opts(SecondsFormat::AutoSi, true),
            "created": created.to_rfc3339_opts(SecondsFormat::AutoSi, false),
            "tags": draft.tags,
            "tui_journal": {"version": 1, "id": uid.to_string()},
        });
        if let Some(priority) = draft.priority {
            metadata["tui_journal"]["priority"] = priority.into();
        }
        let document = Self {
            uid,
            created,
            metadata,
            body: draft.content.clone(),
            original_header: None,
        };
        document.with_entry(&Entry::from_draft(0, draft.clone()))
    }

    pub fn parse(text: &str) -> Result<Option<Self>> {
        let text = text.strip_prefix('\u{feff}').unwrap_or(text);
        let Some((opening, remainder)) = text.split_once('\n') else {
            return Ok(None);
        };
        if opening.trim_end_matches('\r') != "---" {
            return Ok(None);
        }
        let header_start = opening.len() + 1;
        let mut offset = header_start;
        let mut header_end = None;
        for line in remainder.split_inclusive('\n') {
            if line.trim_end_matches(['\r', '\n']) == "---" {
                header_end = Some((offset, offset + line.len()));
                break;
            }
            offset += line.len();
            if offset - header_start > MAX_HEADER_BYTES {
                break;
            }
        }
        let Some((end, body_start)) = header_end else {
            ensure!(
                !remainder.contains("tui_journal"),
                "Journal front matter is unterminated or exceeds 1 MiB"
            );
            return Ok(None);
        };
        let header = &text[header_start..end];
        let metadata = match decode_metadata(header) {
            Ok(metadata) => metadata,
            Err(error) if header.contains("tui_journal") => {
                return Err(error).context("Invalid journal YAML front matter");
            }
            Err(_) => return Ok(None),
        };
        if metadata.get("tui_journal").is_none() {
            return Ok(None);
        }
        let parsed = validate_metadata(&metadata)?;
        let document = Self {
            uid: Uuid::parse_str(&parsed.tui_journal.id).context("Invalid journal UUID")?,
            created: parsed.created,
            metadata,
            body: text[body_start..].into(),
            original_header: Some(text[..body_start].into()),
        };
        Ok(Some(document))
    }

    pub fn entry(&self, id: u32) -> Result<Entry> {
        let metadata = validate_metadata(&self.metadata)?;
        let protection = metadata
            .tui_journal
            .protection
            .map(|protection| EncryptedNote {
                version: protection.version,
                identity: protection.identity,
                ciphertext: self.body.clone(),
            });
        let mut entry = Entry::new(
            id,
            metadata.date,
            metadata.title,
            if protection.is_some() {
                String::new()
            } else {
                self.body.clone()
            },
            metadata.tags,
            metadata.tui_journal.priority,
        );
        entry.protection = protection;
        entry.validate_protection()?;
        Ok(entry)
    }

    pub fn with_entry(&self, entry: &Entry) -> Result<Self> {
        entry.validate_protection()?;
        if let Some(protection) = &entry.protection {
            // Unknown front matter can contain private information, too.
            return Ok(Self {
                uid: self.uid,
                created: self.created,
                metadata: json!({
                    "title": entry.title,
                    "date": entry.date.to_rfc3339_opts(SecondsFormat::AutoSi, true),
                    "created": self.created.to_rfc3339_opts(SecondsFormat::AutoSi, false),
                    "tui_journal": {
                        "version": 2,
                        "id": self.uid.to_string(),
                        "protection": {"version": protection.version, "identity": protection.identity},
                    },
                }),
                body: protection.ciphertext.clone(),
                original_header: None,
            });
        }
        let before = self.entry(entry.id)?;
        let mut document = self.clone();
        if before.protection.is_some() {
            document.metadata["tui_journal"] = json!({"version": 1, "id": self.uid.to_string()});
        }
        if before.title != entry.title {
            document.metadata["title"] = entry.title.clone().into();
        }
        if before.date != entry.date {
            document.metadata["date"] = entry
                .date
                .to_rfc3339_opts(SecondsFormat::AutoSi, true)
                .into();
        }
        if before.tags != entry.tags {
            document.metadata["tags"] = json!(entry.tags);
        }
        if before.priority != entry.priority {
            let app = document.metadata["tui_journal"]
                .as_object_mut()
                .context("Invalid journal metadata")?;
            if let Some(priority) = entry.priority {
                app.insert("priority".into(), priority.into());
            } else {
                app.remove("priority");
            }
        }
        if document.metadata != self.metadata {
            document.original_header = None;
        }
        document.body = entry.content.clone();
        document.entry(entry.id)?;
        Ok(document)
    }

    pub fn restore_metadata(&self, entry: &Entry, text: &str) -> Result<Self> {
        ensure!(
            entry.protection.is_none(),
            "Cannot restore plaintext metadata on a protected note"
        );
        let mut original = Self::parse(text)?.context("Missing journal metadata")?;
        ensure!(
            original.entry(entry.id)?.protection.is_none(),
            "Expected plaintext journal metadata"
        );
        if original.uid != self.uid || original.created != self.created {
            original.uid = self.uid;
            original.created = self.created;
            original.metadata["tui_journal"]["id"] = self.uid.to_string().into();
            original.metadata["created"] = self
                .created
                .to_rfc3339_opts(SecondsFormat::AutoSi, false)
                .into();
            original.original_header = None;
        }
        original.with_entry(entry)
    }

    pub fn text(&self) -> Result<String> {
        self.entry(0)?;
        if let Some(header) = &self.original_header {
            let separator = if self.body.is_empty() || header.ends_with('\n') {
                ""
            } else if !header.ends_with('\r') && header.contains("\r\n") {
                "\r\n"
            } else {
                "\n"
            };
            return Ok(format!("{header}{separator}{}", self.body));
        }
        let yaml = serde_saphyr::to_string(&self.metadata)?;
        let decoded = decode_metadata(&yaml)
            .context("Journal metadata cannot be saved in a readable form")?;
        ensure!(
            decoded == self.metadata,
            "Journal metadata could not be serialized without changing its values"
        );
        Ok(format!(
            "---\n{}\n---\n{}",
            yaml.trim_end_matches('\n'),
            self.body
        ))
    }
}

fn decode_metadata(header: &str) -> Result<Value> {
    ensure!(
        header.len() <= MAX_HEADER_BYTES,
        "Journal metadata exceeds 1 MiB"
    );
    let mut options = serde_saphyr::Options::default();
    options.duplicate_keys = serde_saphyr::DuplicateKeyPolicy::Error;
    options.merge_keys = serde_saphyr::MergeKeyPolicy::Error;
    options.reject_unsupported_tags = true;
    options.strict_booleans = true;
    options.no_schema = true;
    options.with_snippet = false;
    if let Some(budget) = &mut options.budget {
        budget.max_depth = 32;
        budget.max_aliases = 0;
        budget.max_anchors = 0;
        budget.max_total_scalar_bytes = MAX_HEADER_BYTES;
        budget.max_nodes = 32_768;
        budget.max_events = 100_000;
    }
    serde_saphyr::from_str_with_options(header, options).map_err(Into::into)
}

fn validate_metadata(value: &Value) -> Result<Metadata> {
    let metadata: Metadata =
        serde_json::from_value(value.clone()).context("Invalid journal metadata fields")?;
    ensure!(
        matches!(metadata.tui_journal.version, 1 | 2),
        "Unsupported journal format version"
    );
    ensure!(
        (metadata.tui_journal.version == 2) == metadata.tui_journal.protection.is_some(),
        "Journal protection does not match its format version"
    );
    ensure!(!metadata.title.is_empty(), "Entry title can't be empty");
    let uid = Uuid::parse_str(&metadata.tui_journal.id).context("Invalid journal UUID")?;
    if uid.is_nil() {
        bail!("Journal UUID must not be nil");
    }
    Ok(metadata)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(body: &str) -> JournalDocument {
        let date = DateTime::parse_from_rfc3339("2020-02-29T12:34:56.123456789Z")
            .unwrap()
            .with_timezone(&Utc);
        JournalDocument::new(
            &EntryDraft::new(
                date,
                "A title: # yes".into(),
                vec!["null".into()],
                Some(u32::MAX),
            )
            .with_content(body.into()),
            DateTime::parse_from_rfc3339("2026-10-23T19:42:18+10:00").unwrap(),
        )
        .unwrap()
    }

    #[test]
    fn round_trips_body_and_all_metadata_without_newline_normalization() {
        for body in [
            "",
            "\n",
            "no final newline",
            "\r\n  spaced \t\r\n",
            "---\nbody\n---",
            "世界 e\u{301} 👩‍💻\n",
        ] {
            let original = document(body);
            let parsed = JournalDocument::parse(&original.text().unwrap())
                .unwrap()
                .unwrap();
            assert_eq!(parsed.entry(17).unwrap(), original.entry(17).unwrap());
            assert_eq!(parsed.uid, original.uid);
            assert_eq!(parsed.created, original.created);
        }
    }

    #[test]
    fn yaml_looking_titles_and_tags_stay_strings() {
        for title in [
            "true",
            "false",
            "null",
            "yes",
            "off",
            "123",
            "1.2",
            "---",
            "title\n---\ncontinued",
        ] {
            let original = document("");
            let mut entry = original.entry(0).unwrap();
            entry.title = title.into();
            entry.tags = vec![title.into(), String::new()];
            let text = original.with_entry(&entry).unwrap().text().unwrap();
            let parsed = JournalDocument::parse(&text).unwrap().unwrap();
            assert_eq!(parsed.entry(0).unwrap(), entry);
        }
    }

    #[test]
    fn largest_writable_header_can_be_read_back() {
        let mut original = document("body");
        original.metadata["padding"] = "x".repeat(MAX_HEADER_BYTES).into();
        let overhead =
            serde_saphyr::to_string(&original.metadata).unwrap().len() - MAX_HEADER_BYTES;
        original.metadata["padding"] = "x".repeat(MAX_HEADER_BYTES - overhead).into();
        let text = original.text().unwrap();
        let parsed = JournalDocument::parse(&text).unwrap().unwrap();
        assert_eq!(parsed.metadata, original.metadata);
        original.metadata["padding"] = "x".repeat(MAX_HEADER_BYTES - overhead + 1).into();
        assert!(original.text().is_err());
    }

    #[test]
    fn rejects_generated_headers_that_exceed_the_reader_node_budget() {
        let mut original = document("body");
        original.metadata["tags"] = json!(vec!["tag"; 40_000]);
        assert!(serde_saphyr::to_string(&original.metadata).unwrap().len() < MAX_HEADER_BYTES);
        let error = original.text().unwrap_err();
        assert!(
            error
                .to_string()
                .contains("cannot be saved in a readable form")
        );
    }

    #[test]
    fn body_edits_keep_header_comments_and_metadata_edits_keep_unknown_values() {
        let original = document("old").text().unwrap();
        let original = original.replacen(
            "---\n",
            "---\n# personal comment\ncustom: {nested: [one, 2, true]}\n",
            1,
        );
        let parsed = JournalDocument::parse(&original).unwrap().unwrap();
        let mut entry = parsed.entry(0).unwrap();
        entry.content = "replacement\n\n".into();
        let updated = parsed.with_entry(&entry).unwrap();
        assert_eq!(
            updated.text().unwrap(),
            original.strip_suffix("old").unwrap().to_owned() + &entry.content
        );
        entry.title = "changed title".into();
        let updated = updated.with_entry(&entry).unwrap();
        let reparsed = JournalDocument::parse(&updated.text().unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(reparsed.metadata["custom"], parsed.metadata["custom"]);
        assert_eq!(reparsed.uid, parsed.uid);
        assert_eq!(reparsed.created, parsed.created);
        assert_eq!(reparsed.entry(0).unwrap(), entry);
    }

    #[test]
    fn supports_crlf_headers_and_optional_metadata() {
        let mut doc = document("body\r\n");
        doc.metadata.as_object_mut().unwrap().remove("tags");
        doc.metadata["tui_journal"]
            .as_object_mut()
            .unwrap()
            .remove("priority");
        let serialized = doc.text().unwrap();
        let (header, body) = serialized.rsplit_once("---\n").unwrap();
        let crlf = format!("{}---\r\n{body}", header.replace('\n', "\r\n"));
        let parsed = JournalDocument::parse(&crlf).unwrap().unwrap();
        assert_eq!(parsed.entry(0).unwrap().content, "body\r\n");
        assert!(parsed.entry(0).unwrap().tags.is_empty());
        assert_eq!(parsed.entry(0).unwrap().priority, None);
    }

    #[test]
    fn body_edit_after_eof_delimiter_keeps_front_matter_parseable() {
        for newline in ["\n", "\r\n"] {
            let header = document("")
                .text()
                .unwrap()
                .trim_end_matches('\n')
                .replace('\n', newline);
            let parsed = JournalDocument::parse(&header).unwrap().unwrap();
            assert_eq!(parsed.text().unwrap(), header);
            let mut entry = parsed.entry(0).unwrap();
            entry.content = "new body\n---\n".into();
            let saved = parsed.with_entry(&entry).unwrap().text().unwrap();
            assert_eq!(saved, format!("{header}{newline}{}", entry.content));
            let reparsed = JournalDocument::parse(&saved).unwrap().unwrap();
            assert_eq!(reparsed.entry(0).unwrap(), entry);
        }
    }

    #[test]
    fn rejects_invalid_fields_duplicates_aliases_and_special_tags() {
        let good = document("body").text().unwrap();
        for (key, value) in [
            ("title", json!(false)),
            ("title", json!("")),
            ("date", json!("bad")),
            ("created", json!("2026-10-23")),
            ("tags", json!("one,two")),
        ] {
            let mut doc = document("body");
            doc.metadata[key] = value;
            assert!(doc.text().is_err(), "{key}");
        }
        for (key, value) in [
            ("version", json!(2)),
            ("id", json!("bad")),
            ("priority", json!(-1)),
        ] {
            let mut doc = document("body");
            doc.metadata["tui_journal"][key] = value;
            assert!(doc.text().is_err(), "{key}");
        }
        for extra in [
            "title: duplicate\n",
            "custom: &a [one]\nagain: *a\n",
            "custom: !include /etc/passwd\n",
            "custom: .nan\n",
            "custom: {a: 1, a: 2}\n",
            "custom: {1: numeric, \"1\": string}\n",
            "custom: {true: boolean}\n",
            "custom: {[one, two]: sequence}\n",
        ] {
            assert!(
                JournalDocument::parse(&good.replacen("---\n", &format!("---\n{extra}"), 1))
                    .is_err(),
                "{extra}"
            );
        }
    }

    #[test]
    fn distinguishes_unrelated_text_from_damaged_managed_files() {
        for unrelated in [
            "plain text",
            "---\nnormal: metadata\n---\nbody",
            "---\nbroken: [\n---\nbody",
            "---\nno terminator",
        ] {
            assert!(JournalDocument::parse(unrelated).unwrap().is_none());
        }
        for damaged in [
            "---\ntui_journal: {",
            "---\ntui_journal: [\n---\nbody",
            "---\ntui_journal: 1\n---\nbody",
        ] {
            assert!(JournalDocument::parse(damaged).is_err());
        }
        let mut good = document("---\n").text().unwrap();
        good.insert_str(4, &format!("huge: {}\n", "x".repeat(MAX_HEADER_BYTES)));
        assert!(JournalDocument::parse(&good).is_err());
    }
}

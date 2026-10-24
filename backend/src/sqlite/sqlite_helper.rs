use anyhow::Context;
use chrono::{DateTime, Utc};
use sqlx::FromRow;

use crate::Entry;

/// Helper to decode aggregated tags and the versioned protection envelope.
#[derive(FromRow)]
pub(crate) struct EntryIntermediate {
    pub id: u32,
    pub date: DateTime<Utc>,
    pub title: String,
    pub content: String,
    pub priority: Option<u32>,
    pub tags: String,
    pub protection: Option<String>,
}

impl TryFrom<EntryIntermediate> for Entry {
    type Error = anyhow::Error;

    fn try_from(value: EntryIntermediate) -> Result<Self, Self::Error> {
        let entry = Entry {
            id: value.id,
            date: value.date,
            title: value.title,
            content: value.content,
            priority: value.priority,
            tags: serde_json::from_str(&value.tags).context("Invalid entry tags")?,
            protection: value
                .protection
                .as_deref()
                .map(serde_json::from_str)
                .transpose()
                .context("Invalid note protection record")?,
        };
        entry.validate_protection()?;
        Ok(entry)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn sample_intermediate(tags: &str) -> EntryIntermediate {
        EntryIntermediate {
            id: 4,
            date: Utc.with_ymd_and_hms(2024, 3, 4, 5, 6, 7).unwrap(),
            title: String::from("Title"),
            content: String::from("Content"),
            priority: Some(2),
            tags: tags.into(),
            protection: None,
        }
    }

    #[test]
    fn empty_tags_stay_empty() {
        let entry = Entry::try_from(sample_intermediate("[]")).unwrap();
        assert!(entry.tags.is_empty());
    }

    #[test]
    fn tags_preserve_commas_and_empty_values() {
        let entry = Entry::try_from(sample_intermediate(r#"["rust,tests","","sqlite"]"#)).unwrap();
        assert_eq!(entry.tags, vec!["rust,tests", "", "sqlite"]);
    }

    #[test]
    fn corrupt_protection_fails_closed() {
        let mut value = sample_intermediate("[]");
        value.protection = Some("{}".into());
        assert!(Entry::try_from(value).is_err());
    }
}

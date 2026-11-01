use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Mood {
    Low,
    Average,
    High,
}

impl Mood {
    pub const ALL: [Self; 3] = [Self::Low, Self::Average, Self::High];

    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Average => "Average",
            Self::High => "High",
        }
    }

    pub fn marker(self) -> &'static str {
        match self {
            Self::Low => "[-]",
            Self::Average => "[~]",
            Self::High => "[+]",
        }
    }
}

impl Zeroize for Mood {
    fn zeroize(&mut self) {
        *self = Self::Low;
    }
}

/// A snapshot of the chosen prompt, independent of later mood or catalog changes.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalPrompt {
    pub id: String,
    pub mood: Mood,
    pub topic: String,
    pub text: String,
}

impl std::fmt::Debug for JournalPrompt {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("JournalPrompt { [REDACTED] }")
    }
}

impl Zeroize for JournalPrompt {
    fn zeroize(&mut self) {
        self.id.zeroize();
        self.mood.zeroize();
        self.topic.zeroize();
        self.text.zeroize();
    }
}

impl Drop for JournalPrompt {
    fn drop(&mut self) {
        self.zeroize();
    }
}

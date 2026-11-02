use anyhow::{Result, anyhow, ensure};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use super::{
    ActiveText, CursorMove, DateTime, EntryPopup, EntryTimeFormat, TextArea, Utc,
    tags::{TagsPopup, TagsSnapshot},
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TextSnapshot {
    lines: Vec<String>,
    cursor: (usize, usize),
}

impl Drop for TextSnapshot {
    fn drop(&mut self) {
        self.lines.zeroize();
    }
}

impl TextSnapshot {
    fn capture(text: &TextArea<'_>) -> Self {
        let tui_textarea::DataCursor(row, column) = text.cursor();
        Self {
            lines: text.lines().to_vec(),
            cursor: (row, column),
        }
    }

    fn restore(mut self) -> Result<TextArea<'static>> {
        ensure!(!self.lines.is_empty(), "Invalid entry form snapshot");
        let mut text = TextArea::new(std::mem::take(&mut self.lines));
        text.move_cursor(CursorMove::Jump(
            self.cursor.0.min(u16::MAX as usize) as u16,
            self.cursor.1.min(u16::MAX as usize) as u16,
        ));
        Ok(text)
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct EntrySnapshot {
    version: u32,
    title: TextSnapshot,
    date: TextSnapshot,
    tags: TextSnapshot,
    priority: TextSnapshot,
    show_priority: bool,
    #[serde(default)]
    show_mood: bool,
    #[serde(default)]
    mood: Option<backend::Mood>,
    #[serde(default)]
    mood_focus: usize,
    #[serde(default)]
    show_prompt: bool,
    #[serde(default = "default_include_prompt")]
    include_prompt: bool,
    #[serde(default = "default_include_prompt")]
    prompt_focus_yes: bool,
    #[serde(default)]
    time_format: Option<EntryTimeFormat>,
    is_edit_entry: bool,
    original_date: Option<DateTime<Utc>>,
    active: u8,
    tag_picker: Option<TagsSnapshot>,
}

fn default_include_prompt() -> bool {
    true
}

impl EntryPopup<'_> {
    pub(in crate::app::ui) fn snapshot(&self) -> Result<String> {
        serde_json::to_string(&EntrySnapshot {
            version: 1,
            title: TextSnapshot::capture(&self.title_txt),
            date: TextSnapshot::capture(&self.date_txt),
            tags: TextSnapshot::capture(&self.tags_txt),
            priority: TextSnapshot::capture(&self.priority_txt),
            show_priority: self.show_priority,
            show_mood: self.show_mood,
            mood: self.mood,
            mood_focus: self.mood_focus,
            show_prompt: self.show_prompt,
            include_prompt: self.include_prompt,
            prompt_focus_yes: self.prompt_focus_yes,
            time_format: self.time_format,
            is_edit_entry: self.is_edit_entry,
            original_date: self.original_date,
            active: match self.active_txt {
                ActiveText::Title => 0,
                ActiveText::Date => 1,
                ActiveText::Tags => 2,
                ActiveText::Priority => 3,
                ActiveText::Mood => 4,
                ActiveText::Prompt => 5,
            },
            tag_picker: self.tags_popup.as_ref().map(TagsPopup::snapshot),
        })
        .map_err(|_| anyhow!("Could not capture entry form"))
    }

    pub(in crate::app::ui) fn from_snapshot(text: &str) -> Result<Self> {
        let snapshot: EntrySnapshot =
            serde_json::from_str(text).map_err(|_| anyhow!("Invalid entry form snapshot"))?;
        ensure!(
            snapshot.version == 1,
            "Unsupported entry form snapshot version"
        );
        ensure!(
            !snapshot.show_prompt || (snapshot.show_mood && !snapshot.is_edit_entry),
            "Invalid entry form snapshot"
        );
        let active_txt = match snapshot.active {
            0 => ActiveText::Title,
            1 => ActiveText::Date,
            2 => ActiveText::Tags,
            3 => ActiveText::Priority,
            4 if snapshot.show_mood => ActiveText::Mood,
            5 if snapshot.show_prompt => ActiveText::Prompt,
            _ => return Err(anyhow!("Invalid entry form snapshot")),
        };
        let mut popup = Self {
            title_txt: snapshot.title.restore()?,
            date_txt: snapshot.date.restore()?,
            tags_txt: snapshot.tags.restore()?,
            priority_txt: snapshot.priority.restore()?,
            show_priority: snapshot.show_priority,
            show_mood: snapshot.show_mood,
            mood: snapshot.mood,
            mood_focus: snapshot.mood_focus.min(2),
            show_prompt: snapshot.show_prompt,
            include_prompt: snapshot.include_prompt,
            prompt_focus_yes: snapshot.prompt_focus_yes,
            time_format: snapshot.time_format,
            is_edit_entry: snapshot.is_edit_entry,
            original_date: snapshot.original_date,
            active_txt,
            title_err_msg: String::new(),
            date_err_msg: String::new(),
            tags_err_msg: String::new(),
            priority_err_msg: String::new(),
            tags_popup: snapshot.tag_picker.map(TagsPopup::from_snapshot),
        };
        popup.validate_all();
        Ok(popup)
    }
}

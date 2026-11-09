use std::borrow::Cow;

use crossterm::event::{KeyCode, KeyModifiers};
use tui_textarea::DataCursor;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthChar;

use super::super::text_mask::{is_word_character, is_word_connector};
use super::{Editor, EditorMode, Input};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct VisibleWord {
    row: usize,
    start: usize,
    end: usize,
}

impl VisibleWord {
    fn contains_cursor(self, DataCursor(row, col): DataCursor) -> bool {
        self.row == row && self.start <= col && col <= self.end
    }
}

impl Editor<'_> {
    pub(super) fn track_visible_word(
        &mut self,
        input: &Input,
        changed: bool,
        previous_cursor: DataCursor,
    ) {
        let previous_word = self.visible_word.take();
        if self.text_mask.is_none()
            || !self.is_active
            || self.mode != EditorMode::Insert
            || !changed
            || !(input.modifiers - KeyModifiers::SHIFT).is_empty()
        {
            return;
        }

        let DataCursor(row, col) = self.text_area.cursor();
        let previous_is_current = previous_word
            .is_some_and(|word| word.contains_cursor(previous_cursor) && word.row == row);
        let anchor = match input.key_code {
            KeyCode::Char(c) if is_word_character(c) => col.checked_sub(1),
            KeyCode::Char(c)
                if previous_is_current
                    && (is_word_connector(c)
                        || (!c.is_control() && UnicodeWidthChar::width(c) == Some(0))) =>
            {
                col.checked_sub(1)
            }
            KeyCode::Backspace if previous_is_current => previous_word.and_then(|word| {
                (previous_cursor.1 > word.start && col >= word.start)
                    .then_some(if col > word.start { col - 1 } else { col })
            }),
            KeyCode::Delete if previous_is_current => previous_word.and_then(|word| {
                (previous_cursor.1 < word.end && previous_cursor.1 == col)
                    .then_some(if col > word.start { col - 1 } else { col })
            }),
            _ => None,
        };
        self.visible_word = anchor.and_then(|anchor| self.word_containing(row, anchor));
    }

    fn word_containing(&self, row: usize, anchor: usize) -> Option<VisibleWord> {
        let line = self.text_area.lines().get(row)?;
        let mut column = 0;
        let mut start = 0;
        let mut has_word_character = false;
        let mut contains_anchor = false;
        for text in line.graphemes(true) {
            let end = column + text.chars().count();
            let word_character = text.chars().any(is_word_character);
            if word_character || text.chars().all(is_word_connector) {
                has_word_character |= word_character;
                contains_anchor |= column <= anchor && anchor < end;
            } else {
                if contains_anchor {
                    break;
                }
                if column <= anchor && anchor < end {
                    return None;
                }
                start = end;
                has_word_character = false;
            }
            column = end;
        }
        (contains_anchor && has_word_character).then_some(VisibleWord {
            row,
            start,
            end: column,
        })
    }

    pub(super) fn displayed_glyph<'a>(
        &self,
        row: usize,
        start: usize,
        end: usize,
        text: Cow<'a, str>,
    ) -> Cow<'a, str> {
        let Some(mask) = self.text_mask else {
            return text;
        };
        if self.is_active
            && self.mode == EditorMode::Insert
            && self
                .visible_word
                .is_some_and(|word| word.row == row && word.start <= start && end <= word.end)
        {
            return text;
        }
        let salt =
            (row as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ (start as u64).rotate_left(23);
        Cow::Owned(mask.scramble(text.as_ref(), salt))
    }
}

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct TextMask {
    seed: u64,
}

impl TextMask {
    pub(super) fn new() -> Self {
        Self {
            seed: rand::random(),
        }
    }

    pub(super) fn scramble(&self, text: &str, salt: u64) -> String {
        const LETTERS: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
        let mut result = String::with_capacity(text.len());
        for (index, glyph) in text.graphemes(true).enumerate() {
            if !glyph.chars().any(is_word_character) {
                result.push_str(glyph);
                continue;
            }
            let mut remaining = glyph.width();
            let mut hasher = DefaultHasher::new();
            (self.seed, salt, index).hash(&mut hasher);
            let mut choice = hasher.finish() as usize % LETTERS.len();
            while remaining > 0 {
                let wide = remaining >= 2;
                let replacement = loop {
                    let ascii = char::from(LETTERS[choice]);
                    let replacement = if wide {
                        char::from_u32(ascii as u32 + 0xfee0).expect("fullwidth Latin character")
                    } else {
                        ascii
                    };
                    choice = (choice + 1) % LETTERS.len();
                    if !glyph.starts_with(replacement) {
                        break replacement;
                    }
                };
                result.push(replacement);
                remaining -= if wide { 2 } else { 1 };
            }
        }
        result
    }
}

pub(super) fn is_word_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

pub(super) fn is_word_connector(character: char) -> bool {
    matches!(character, '\'' | '’' | '-' | '‐' | '‑')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scramble_preserves_spacing_punctuation_and_grapheme_widths() {
        let mask = TextMask { seed: 72 };
        let source = "Hi_42, don't! well-being.\n\té e\u{301} 家族 क्ष 👨‍👩‍👧‍👦";
        let scrambled = mask.scramble(source, 5);
        let source_glyphs: Vec<_> = source.graphemes(true).collect();
        let scrambled_glyphs: Vec<_> = scrambled.graphemes(true).collect();
        assert_eq!(source_glyphs.len(), scrambled_glyphs.len());
        for (original, replacement) in source_glyphs.into_iter().zip(scrambled_glyphs) {
            assert_eq!(original.width(), replacement.width(), "{original:?}");
            if original.chars().any(is_word_character) {
                assert_ne!(original, replacement);
            } else {
                assert_eq!(original, replacement);
            }
        }
    }

    #[test]
    fn repeated_draws_are_stable_and_sessions_use_separate_replacements() {
        let mask = TextMask { seed: 42 };
        let source = "A longer piece of private text with punctuation.";
        let first = mask.scramble(source, 8);
        assert_eq!(first, mask.scramble(source, 8));
        assert_ne!(first, mask.scramble(source, 9));
        assert_ne!(first, TextMask { seed: 43 }.scramble(source, 8));
    }
}

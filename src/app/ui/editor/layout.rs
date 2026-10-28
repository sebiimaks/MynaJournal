use std::borrow::Cow;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;
use zeroize::Zeroizing;

pub(super) struct WrappedLayout {
    pub rows: Vec<VisualRow>,
    source_lines: Vec<Zeroizing<String>>,
    width: usize,
    tab_width: usize,
}

impl std::fmt::Debug for WrappedLayout {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("WrappedLayout")
            .field("rows", &self.rows.len())
            .field("width", &self.width)
            .field("tab_width", &self.tab_width)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
pub(super) struct VisualRow {
    pub source_row: usize,
    pub start_col: usize,
    pub end_col: usize,
    pub glyphs: Vec<VisualGlyph>,
}

#[derive(Debug)]
pub(super) struct VisualGlyph {
    pub source_start: usize,
    pub source_end: usize,
    text: GlyphText,
    pub column: usize,
    pub width: usize,
}

#[derive(Debug)]
enum GlyphText {
    Source { start: usize, end: usize },
    Replacement,
    Spaces(usize),
}

struct SourceGlyph<'a> {
    text: &'a str,
    byte: usize,
    start: usize,
    end: usize,
    whitespace: bool,
    break_after: bool,
    crlf: bool,
}

impl WrappedLayout {
    /// Source columns count Unicode scalar values, as `TextArea` does. Display
    /// columns count cells, and never divide a grapheme. A zero-width viewport
    /// uses a virtual one-cell layout; its caller must skip drawing it.
    pub fn new(lines: &[String], width: usize, tab_width: usize) -> Self {
        Self::new_region(lines, 0, lines.len(), width, tab_width)
    }

    pub fn unwrapped_window(lines: &[String], has_following_line: bool, tab_width: usize) -> Self {
        Self::new_region(
            lines,
            0,
            lines.len() + usize::from(has_following_line),
            usize::MAX,
            tab_width,
        )
    }

    fn new_region(
        lines: &[String],
        first_row: usize,
        source_count: usize,
        width: usize,
        tab_width: usize,
    ) -> Self {
        let width = width.max(1);
        let tab_width = tab_width.max(1);
        let mut rows = Vec::new();

        for (offset, line) in lines.iter().enumerate() {
            let source_row = first_row + offset;
            let mut source_col = 0;
            let source: Vec<_> = line
                .grapheme_indices(true)
                .map(|(byte, text)| {
                    let start = source_col;
                    source_col += text.chars().count();
                    SourceGlyph {
                        text,
                        byte,
                        start,
                        end: source_col,
                        whitespace: text.chars().all(char::is_whitespace),
                        break_after: text
                            .chars()
                            .all(|c| c.is_whitespace() && !matches!(c, '\u{a0}' | '\u{202f}')),
                        crlf: text == "\r"
                            && byte + text.len() == line.len()
                            && source_row + 1 < source_count,
                    }
                })
                .collect();

            let mut start = 0;
            while start < source.len() {
                let mut glyphs = Vec::new();
                let mut column = 0;
                let mut end = start;
                let mut last_break = None;
                let mut saw_word = false;

                while let Some(glyph) = source.get(end) {
                    let rendered = render_glyph(glyph, column, width, tab_width);
                    if rendered.width > width - column {
                        if !glyph.whitespace
                            && let Some(break_end) = last_break
                        {
                            end = break_end;
                            glyphs.truncate(end - start);
                        }
                        break;
                    }

                    column += rendered.width;
                    glyphs.push(rendered);
                    saw_word |= !glyph.whitespace;
                    end += 1;
                    if saw_word && glyph.break_after {
                        last_break = Some(end);
                    }
                }

                rows.push(VisualRow {
                    source_row,
                    start_col: source[start].start,
                    end_col: source[end - 1].end,
                    glyphs,
                });
                start = end;
            }

            // Every source line has a drawable end cursor, including a line
            // whose final glyph exactly fills the viewport.
            if source.is_empty()
                || rows.last().is_some_and(|row| {
                    row.glyphs
                        .last()
                        .is_some_and(|glyph| glyph.column + glyph.width == width)
                })
            {
                rows.push(VisualRow {
                    source_row,
                    start_col: source_col,
                    end_col: source_col,
                    glyphs: Vec::new(),
                });
            }
        }

        if rows.is_empty() && source_count == 0 {
            rows.push(VisualRow {
                source_row: 0,
                start_col: 0,
                end_col: 0,
                glyphs: Vec::new(),
            });
        }

        Self {
            rows,
            source_lines: lines
                .iter()
                .map(|line| Zeroizing::new(line.clone()))
                .collect(),
            width,
            tab_width,
        }
    }

    pub fn update(&mut self, lines: &[String], width: usize, tab_width: usize) {
        if self.width != width.max(1)
            || self.tab_width != tab_width.max(1)
            || self.source_lines.is_empty()
            || lines.is_empty()
        {
            *self = Self::new(lines, width, tab_width);
            return;
        }
        let old_len = self.source_lines.len();
        let mut prefix = 0;
        while prefix < old_len.min(lines.len())
            && self.source_lines[prefix].as_str() == lines[prefix]
            && (prefix + 1 == old_len) == (prefix + 1 == lines.len())
        {
            prefix += 1;
        }
        if prefix == old_len && prefix == lines.len() {
            return;
        }
        let mut suffix = 0;
        while suffix < (old_len - prefix).min(lines.len() - prefix)
            && self.source_lines[old_len - suffix - 1].as_str() == lines[lines.len() - suffix - 1]
        {
            suffix += 1;
        }
        let old_suffix = old_len - suffix;
        let new_suffix = lines.len() - suffix;
        let middle = Self::new_region(
            &lines[prefix..new_suffix],
            prefix,
            lines.len(),
            width,
            tab_width,
        );
        let mut rows = Vec::with_capacity(self.rows.len() + middle.rows.len());
        let mut suffix_rows = Vec::new();
        for mut row in std::mem::take(&mut self.rows) {
            if row.source_row < prefix {
                rows.push(row);
            } else if row.source_row >= old_suffix {
                row.source_row = new_suffix + (row.source_row - old_suffix);
                suffix_rows.push(row);
            }
        }
        rows.extend(middle.rows);
        rows.extend(suffix_rows);
        let mut source_lines = Vec::with_capacity(lines.len());
        let mut suffix_lines = Vec::new();
        for (index, line) in std::mem::take(&mut self.source_lines)
            .into_iter()
            .enumerate()
        {
            if index < prefix {
                source_lines.push(line);
            } else if index >= old_suffix {
                suffix_lines.push(line);
            }
        }
        source_lines.extend(middle.source_lines);
        source_lines.extend(suffix_lines);
        self.rows = rows;
        self.source_lines = source_lines;
    }

    pub fn glyph_text<'a>(&'a self, row: &VisualRow, glyph: &VisualGlyph) -> Cow<'a, str> {
        match glyph.text {
            GlyphText::Source { start, end } => {
                Cow::Borrowed(&self.source_lines[row.source_row][start..end])
            }
            GlyphText::Replacement => Cow::Borrowed("�"),
            GlyphText::Spaces(count) => Cow::Owned(" ".repeat(count)),
        }
    }

    pub fn is_line_end(&self, visual_row: usize) -> bool {
        self.rows
            .get(visual_row + 1)
            .is_none_or(|next| next.source_row != self.rows[visual_row].source_row)
    }

    /// Wrap boundaries belong to the following row. A cursor inside a
    /// multi-scalar grapheme maps to that whole glyph's starting cell.
    pub fn source_to_visual(&self, source: (usize, usize)) -> (usize, usize) {
        let index = self
            .rows
            .partition_point(|row| (row.source_row, row.start_col) <= source)
            .saturating_sub(1);
        let row = &self.rows[index];
        let column = row
            .glyphs
            .iter()
            .find(|glyph| source.0 == row.source_row && source.1 < glyph.source_end)
            .map(|glyph| glyph.column)
            .unwrap_or_else(|| row.glyphs.last().map_or(0, |g| g.column + g.width));
        (index, column)
    }

    /// A visual target past a short continuation row lands on its final glyph.
    /// Returning its end boundary would jump to the next visual row instead.
    pub fn visual_to_source(&self, visual_row: usize, desired_col: usize) -> (usize, usize) {
        let index = visual_row.min(self.rows.len() - 1);
        let row = &self.rows[index];
        let source_col = row
            .glyphs
            .iter()
            .find(|glyph| desired_col < glyph.column + glyph.width)
            .map(|glyph| glyph.source_start)
            .unwrap_or_else(|| {
                if self
                    .rows
                    .get(index + 1)
                    .is_some_and(|next| next.source_row == row.source_row)
                {
                    row.glyphs.last().map_or(row.start_col, |g| g.source_start)
                } else {
                    row.end_col
                }
            });
        (row.source_row, source_col)
    }

    pub fn row_count(&self) -> usize {
        self.rows.len()
    }
}

fn render_glyph(
    source: &SourceGlyph<'_>,
    column: usize,
    width: usize,
    tab_width: usize,
) -> VisualGlyph {
    let is_tab = source.text == "\t";
    let glyph_width = if is_tab {
        tab_width - column % tab_width
    } else {
        source.text.width()
    };
    let unsafe_control = !is_tab && source.text.chars().any(char::is_control);
    // Keep CRLF's source column editable without displaying an invalid-character marker.
    let (text, glyph_width) = if source.crlf {
        (GlyphText::Spaces(1), 1)
    } else if unsafe_control || glyph_width == 0 || glyph_width > width {
        (GlyphText::Replacement, 1)
    } else if is_tab {
        (GlyphText::Spaces(glyph_width), glyph_width)
    } else {
        (
            GlyphText::Source {
                start: source.byte,
                end: source.byte + source.text.len(),
            },
            glyph_width,
        )
    };
    VisualGlyph {
        source_start: source.start,
        source_end: source.end,
        text,
        column,
        width: glyph_width,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout(lines: &[&str], width: usize) -> WrappedLayout {
        WrappedLayout::new(
            &lines
                .iter()
                .map(|line| (*line).to_owned())
                .collect::<Vec<_>>(),
            width,
            4,
        )
    }

    fn text_rows(layout: &WrappedLayout) -> Vec<String> {
        layout
            .rows
            .iter()
            .map(|row| {
                row.glyphs
                    .iter()
                    .map(|g| layout.glyph_text(row, g))
                    .collect()
            })
            .collect()
    }

    #[test]
    fn incremental_reflow_matches_fresh_layout_after_edits_and_resize() {
        let versions: &[&[&str]] = &[
            &["first line", "middle\t中 e\u{301}", "last"],
            &["first line", "edited 中", "last"],
            &["first line", "inserted", "edited 中", "last"],
            &["first line", "last"],
            &["first line\r"],
            &["first line\r", ""],
            &["first line\r"],
            &[],
            &[""],
        ];
        let mut cached = layout(versions[0], 8);
        for width in [8, 1, 12] {
            for tab_width in [4, 8] {
                for lines in versions {
                    let lines: Vec<String> = lines.iter().map(|line| (*line).to_owned()).collect();
                    cached.update(&lines, width, tab_width);
                    let fresh = WrappedLayout::new(&lines, width, tab_width);
                    assert_eq!(text_rows(&cached), text_rows(&fresh));
                    assert_eq!(cached.row_count(), fresh.row_count());
                    for (row, line) in lines.iter().enumerate() {
                        for col in 0..=line.chars().count() {
                            assert_eq!(
                                cached.source_to_visual((row, col)),
                                fresh.source_to_visual((row, col))
                            );
                        }
                    }
                    for row in 0..cached.row_count() {
                        for col in 0..=width {
                            assert_eq!(
                                cached.visual_to_source(row, col),
                                fresh.visual_to_source(row, col)
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn editing_a_middle_line_reuses_unchanged_prefix_and_suffix_layouts() {
        let mut cached = layout(&["first", "old", "last"], 20);
        let first_glyphs = cached.rows[0].glyphs.as_ptr();
        let last_glyphs = cached.rows[2].glyphs.as_ptr();
        cached.update(
            &["first".into(), "new".into(), "extra".into(), "last".into()],
            20,
            4,
        );
        assert_eq!(text_rows(&cached), ["first", "new", "extra", "last"]);
        assert_eq!(cached.rows[0].glyphs.as_ptr(), first_glyphs);
        assert_eq!(cached.rows[3].glyphs.as_ptr(), last_glyphs);
        assert_eq!(cached.rows[3].source_row, 3);
    }

    fn assert_source_coverage(lines: &[&str], layout: &WrappedLayout) {
        for (source_row, source) in lines.iter().enumerate() {
            let mut next = 0;
            for row in layout.rows.iter().filter(|r| r.source_row == source_row) {
                assert_eq!(row.start_col, next);
                for glyph in &row.glyphs {
                    assert_eq!(glyph.source_start, next);
                    assert!(glyph.source_end > glyph.source_start);
                    next = glyph.source_end;
                }
                assert_eq!(row.end_col, next);
            }
            assert_eq!(next, source.chars().count());
        }
    }

    #[test]
    fn wraps_words_without_dropping_separators() {
        let wrapped = layout(&["hello world again"], 10);
        assert_eq!(text_rows(&wrapped), ["hello ", "world ", "again"]);
        assert_source_coverage(&["hello world again"], &wrapped);
    }

    #[test]
    fn exact_width_separator_stays_visible_without_a_whitespace_only_row() {
        let wrapped = layout(&["hello world"], 5);
        assert_eq!(text_rows(&wrapped), ["hello", " worl", "d"]);
        assert_eq!(wrapped.source_to_visual((0, 5)), (1, 0));
        assert_source_coverage(&["hello world"], &wrapped);
    }

    #[test]
    fn preserves_leading_repeated_and_trailing_spaces() {
        let line = "   alpha     beta  ";
        let wrapped = layout(&[line], 6);
        assert_eq!(text_rows(&wrapped).concat(), line);
        assert_source_coverage(&[line], &wrapped);
    }

    #[test]
    fn full_final_row_has_a_drawable_end_cursor() {
        let wrapped = layout(&["hello"], 5);
        assert_eq!(text_rows(&wrapped), ["hello", ""]);
        assert_eq!(wrapped.source_to_visual((0, 5)), (1, 0));
        assert_eq!(wrapped.visual_to_source(1, 100), (0, 5));
    }

    #[test]
    fn empty_lines_and_final_newline_remain_distinct() {
        let wrapped = layout(&["abc", "", ""], 3);
        assert_eq!(text_rows(&wrapped), ["abc", "", "", ""]);
        assert_eq!(wrapped.source_to_visual((0, 3)), (1, 0));
        assert_eq!(wrapped.source_to_visual((1, 0)), (2, 0));
        assert_eq!(wrapped.source_to_visual((2, 0)), (3, 0));
    }

    #[test]
    fn crlf_uses_a_blank_cell_without_removing_its_source_column() {
        let lines = ["abc\r", "last\r", ""];
        let wrapped = layout(&lines, 10);
        assert_eq!(text_rows(&wrapped), ["abc ", "last ", ""]);
        assert_source_coverage(&lines, &wrapped);
        assert_eq!(wrapped.source_to_visual((0, 3)), (0, 3));
        assert_eq!(wrapped.source_to_visual((0, 4)), (0, 4));
        assert_eq!(text_rows(&layout(&["abc\r"], 10)), ["abc�"]);
    }

    #[test]
    fn wide_and_joined_graphemes_stay_whole() {
        let wrapped = layout(&["中a👨‍👩‍👧‍👦e\u{301}"], 3);
        assert_eq!(text_rows(&wrapped), ["中a", "👨‍👩‍👧‍👦e\u{301}", ""]);
        assert_eq!(wrapped.source_to_visual((0, 2)), (1, 0));
        assert_eq!(wrapped.source_to_visual((0, 4)), (1, 0));
        assert_eq!(wrapped.source_to_visual((0, 9)), (1, 2));
        assert_eq!(wrapped.source_to_visual((0, 10)), (1, 2));
        assert_source_coverage(&["中a👨‍👩‍👧‍👦e\u{301}"], &wrapped);
    }

    #[test]
    fn tabs_use_visual_tab_stops_and_preserve_source_indices() {
        let wrapped = layout(&["a\tb\tc"], 8);
        assert_eq!(text_rows(&wrapped), ["a   b   ", "c"]);
        assert_eq!(wrapped.source_to_visual((0, 2)), (0, 4));
        assert_eq!(wrapped.visual_to_source(0, 2), (0, 1));
        assert_source_coverage(&["a\tb\tc"], &wrapped);
    }

    #[test]
    fn tabs_are_remeasured_after_a_word_moves_to_the_next_row() {
        let wrapped = layout(&["aa abcdef\tz"], 8);
        assert_eq!(text_rows(&wrapped), ["aa ", "abcdef  ", "z"]);
        assert_eq!(wrapped.source_to_visual((0, 9)), (1, 6));
        assert_eq!(wrapped.source_to_visual((0, 10)), (2, 0));
    }

    #[test]
    fn narrow_rows_use_visible_replacements_without_losing_source() {
        let lines = ["中\t\r\u{301}\u{200d}"];
        let wrapped = layout(&lines, 1);
        assert!(wrapped.rows.iter().all(|row| row.glyphs.iter().all(|g| {
            let text = wrapped.glyph_text(row, g);
            g.width == 1 && text == "�" && !text.chars().any(char::is_control)
        })));
        assert_source_coverage(&lines, &wrapped);
    }

    #[test]
    fn empty_input_and_zero_width_are_safe() {
        let empty = layout(&[], 0);
        assert_eq!(empty.row_count(), 1);
        assert_eq!(empty.source_to_visual((100, 100)), (0, 0));
        assert_eq!(empty.visual_to_source(100, 100), (0, 0));
        let wrapped = layout(&["中ab"], 0);
        assert_eq!(text_rows(&wrapped), ["�", "a", "b", ""]);
    }

    #[test]
    fn past_short_continuation_row_stays_on_that_visual_row() {
        let wrapped = layout(&["hello world"], 8);
        assert_eq!(text_rows(&wrapped), ["hello ", "world"]);
        let source = wrapped.visual_to_source(0, 7);
        assert_eq!(source, (0, 5));
        assert_eq!(wrapped.source_to_visual(source), (0, 5));
    }

    #[test]
    fn source_boundaries_round_trip_and_every_cursor_is_drawable() {
        let lines = [
            "",
            "ascii words   ",
            "中文 abc",
            "e\u{301}👨‍👩‍👧‍👦z",
            "\tindent\t",
            "\r\u{200d}",
        ];
        for width in 1..=12 {
            let wrapped = layout(&lines, width);
            assert_source_coverage(&lines, &wrapped);
            for (source_row, line) in lines.iter().enumerate() {
                let mut source_col = 0;
                for grapheme in line.graphemes(true) {
                    let visual = wrapped.source_to_visual((source_row, source_col));
                    assert!(visual.1 < width);
                    assert_eq!(
                        wrapped.visual_to_source(visual.0, visual.1),
                        (source_row, source_col)
                    );
                    source_col += grapheme.chars().count();
                }
                let visual = wrapped.source_to_visual((source_row, source_col));
                assert!(visual.1 < width);
                assert_eq!(
                    wrapped.visual_to_source(visual.0, visual.1),
                    (source_row, source_col)
                );
            }
            for row in &wrapped.rows {
                let mut next_column = 0;
                for glyph in &row.glyphs {
                    assert_eq!(glyph.column, next_column);
                    assert_eq!(wrapped.glyph_text(row, glyph).width(), glyph.width);
                    next_column += glyph.width;
                }
                assert!(next_column <= width);
            }
        }
    }

    #[test]
    fn large_source_columns_do_not_wrap_at_u16_limit() {
        let line = "a".repeat(70_000);
        let wrapped = layout(&[&line], 80);
        assert_eq!(wrapped.source_to_visual((0, 65_536)), (819, 16));
        assert_eq!(wrapped.visual_to_source(819, 16), (0, 65_536));
        assert_eq!(wrapped.source_to_visual((0, 70_000)), (875, 0));
        assert_eq!(wrapped.visual_to_source(875, 0), (0, 70_000));
    }

    #[test]
    fn mixed_short_inputs_always_have_bounded_rows_and_drawable_positions() {
        let fragments = ["a", " ", "中", "\t", "e\u{301}", "👨‍👩‍👧‍👦", "\r", "\u{200d}"];
        for first in fragments {
            for second in fragments {
                for third in fragments {
                    let line = format!("{first}{second}{third}");
                    for width in 0..=8 {
                        let wrapped = layout(&[&line], width);
                        assert_source_coverage(&[&line], &wrapped);
                        for source_col in 0..=line.chars().count() {
                            let visual = wrapped.source_to_visual((0, source_col));
                            assert!(visual.1 < width.max(1), "{line:?}, width {width}");
                        }
                        for (visual_row, row) in wrapped.rows.iter().enumerate() {
                            for glyph in &row.glyphs {
                                assert!(glyph.column + glyph.width <= width.max(1));
                                assert_eq!(wrapped.glyph_text(row, glyph).width(), glyph.width);
                                assert_eq!(
                                    wrapped.visual_to_source(visual_row, glyph.column),
                                    (0, glyph.source_start)
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}

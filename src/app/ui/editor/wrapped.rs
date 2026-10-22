use super::{Editor, EditorMode, Input, Styles, VerticalDir, layout::WrappedLayout};
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Margin, Rect},
    style::Style,
    symbols,
    widgets::{Block, Borders, Scrollbar, ScrollbarOrientation, ScrollbarState},
};
use tui_textarea::{CursorMove, TextArea};
use unicode_segmentation::UnicodeSegmentation;

#[derive(Default)]
pub(super) struct WrappedView {
    pub width: usize,
    height: usize,
    top: usize,
    goal_column: Option<usize>,
    layout: Option<WrappedLayout>,
}

impl WrappedView {
    pub fn invalidate(&mut self) {
        self.layout = None;
    }

    pub fn clear_goal(&mut self) {
        self.goal_column = None;
    }

    fn layout(&mut self, text: &TextArea<'_>) -> &WrappedLayout {
        self.layout.get_or_insert_with(|| {
            WrappedLayout::new(text.lines(), self.width, usize::from(text.tab_length()))
        })
    }
}

impl Editor<'_> {
    pub fn set_word_wrap(&mut self, enabled: bool) {
        if self.word_wrap != enabled {
            self.word_wrap = enabled;
            self.wrapped = WrappedView::default();
        }
    }

    pub(super) fn handle_wrapped_navigation(&mut self, input: &Input) -> bool {
        if !self.word_wrap || self.wrapped.width == 0 {
            return false;
        }
        let modifiers = input.modifiers - KeyModifiers::SHIFT;
        let select = self.should_extend_selection(input);
        let page = self.wrapped.height.saturating_sub(1).max(1);
        let movement = match input.key_code {
            KeyCode::Up if modifiers.is_empty() => Some((VerticalDir::Up, 1)),
            KeyCode::Down if modifiers.is_empty() => Some((VerticalDir::Down, 1)),
            KeyCode::Char('p') if modifiers == KeyModifiers::CONTROL => Some((VerticalDir::Up, 1)),
            KeyCode::Char('n') if modifiers == KeyModifiers::CONTROL => {
                Some((VerticalDir::Down, 1))
            }
            KeyCode::PageUp => Some((VerticalDir::Up, page)),
            KeyCode::PageDown => Some((VerticalDir::Down, page)),
            KeyCode::Char('v') if modifiers == KeyModifiers::CONTROL => {
                Some((VerticalDir::Down, page))
            }
            KeyCode::Char('v') if modifiers == KeyModifiers::ALT => Some((VerticalDir::Up, page)),
            KeyCode::Char('f' | 'b' | 'd' | 'u')
                if modifiers == KeyModifiers::CONTROL && !self.is_insert_mode() =>
            {
                let dir = if matches!(input.key_code, KeyCode::Char('f' | 'd')) {
                    VerticalDir::Down
                } else {
                    VerticalDir::Up
                };
                let distance = if matches!(input.key_code, KeyCode::Char('d' | 'u')) {
                    (self.wrapped.height / 2).max(1)
                } else {
                    page
                };
                Some((dir, distance))
            }
            KeyCode::Left if modifiers.is_empty() => {
                return self.move_wrapped_horizontal(false, select);
            }
            KeyCode::Right if modifiers.is_empty() => {
                return self.move_wrapped_horizontal(true, select);
            }
            _ => None,
        };
        if let Some((dir, distance)) = movement {
            self.move_wrapped_vertical(dir, distance, select);
            true
        } else {
            false
        }
    }

    pub(super) fn move_wrapped_vertical(
        &mut self,
        dir: VerticalDir,
        distance: usize,
        select: bool,
    ) {
        let cursor = self.text_area.cursor();
        let layout = self.wrapped.layout(&self.text_area);
        let (row, column) = layout.source_to_visual(cursor);
        let last = layout.row_count().saturating_sub(1);
        let target_row = match dir {
            VerticalDir::Up => row.saturating_sub(distance),
            VerticalDir::Down => row.saturating_add(distance).min(last),
        };
        let goal = self.wrapped.goal_column.unwrap_or(column);
        let target = if target_row == row {
            match dir {
                VerticalDir::Up => (0, 0),
                VerticalDir::Down => {
                    let last = self.text_area.lines().len() - 1;
                    (last, self.text_area.lines()[last].chars().count())
                }
            }
        } else {
            self.wrapped
                .layout(&self.text_area)
                .visual_to_source(target_row, goal)
        };
        if target != cursor {
            self.prepare_selection(select);
            self.jump_to_source(target);
            self.wrapped.goal_column = (target_row != row).then_some(goal);
        }
    }

    pub(super) fn move_wrapped_horizontal(&mut self, forward: bool, select: bool) -> bool {
        if !self.word_wrap {
            return false;
        }
        let (row, col) = self.text_area.cursor();
        let line = &self.text_area.lines()[row];
        let mut boundaries = vec![0];
        for glyph in line.graphemes(true) {
            boundaries.push(boundaries.last().unwrap() + glyph.chars().count());
        }
        let target = if forward {
            boundaries
                .into_iter()
                .find(|&boundary| boundary > col)
                .map(|boundary| (row, boundary))
                .or_else(|| (row + 1 < self.text_area.lines().len()).then_some((row + 1, 0)))
        } else {
            boundaries
                .into_iter()
                .rev()
                .find(|&boundary| boundary < col)
                .map(|boundary| (row, boundary))
                .or_else(|| {
                    row.checked_sub(1)
                        .map(|prev| (prev, self.text_area.lines()[prev].chars().count()))
                })
        };
        // A plain arrow also clears a selection when already at the document edge.
        self.prepare_selection(select);
        if let Some(target) = target {
            self.jump_to_source(target);
        }
        self.wrapped.clear_goal();
        true
    }

    fn prepare_selection(&mut self, select: bool) {
        if select {
            if !self.text_area.is_selecting() {
                self.text_area.start_selection();
            }
        } else {
            self.text_area.cancel_selection();
        }
    }

    // TextArea's absolute jump accepts u16 even though its source positions use usize.
    // Finish oversized jumps using relative moves instead of truncating the destination.
    fn jump_to_source(&mut self, (row, col): (usize, usize)) {
        self.text_area.move_cursor(CursorMove::Jump(
            row.min(u16::MAX as usize) as u16,
            col.min(u16::MAX as usize) as u16,
        ));
        while self.text_area.cursor().0 < row {
            self.text_area.move_cursor(CursorMove::Down);
        }
        if self.text_area.cursor().1 > col {
            self.text_area.move_cursor(CursorMove::Head);
        }
        if col > u16::MAX as usize {
            let end = self.text_area.lines()[row].chars().count();
            if end - col < col.saturating_sub(self.text_area.cursor().1) {
                self.text_area.move_cursor(CursorMove::End);
                while self.text_area.cursor().1 > col {
                    self.text_area.move_cursor(CursorMove::Back);
                }
            }
        }
        while self.text_area.cursor().1 < col {
            self.text_area.move_cursor(CursorMove::Forward);
        }
    }

    pub(super) fn render_wrapped(&mut self, frame: &mut Frame, area: Rect, styles: &Styles) {
        let estyles = &styles.editor;
        let border_style = match (self.mode, self.is_active) {
            (EditorMode::Insert, _) => estyles.block_insert,
            (EditorMode::Visual, _) => estyles.block_visual,
            (EditorMode::Normal, true) => estyles.block_normal_active,
            (EditorMode::Normal, false) => estyles.block_normal_inactive,
        };
        let mut title = String::from("Content");
        if self.is_active {
            title.push_str(match self.mode {
                EditorMode::Normal => " - NORMAL",
                EditorMode::Insert => " - EDIT",
                EditorMode::Visual => " - Visual",
            });
        }
        if self.has_unsaved {
            title.push_str(" *");
        }
        let block = Block::default()
            .borders(Borders::ALL)
            .style(border_style)
            .title(title);
        let inner = block.inner(area);
        frame.render_widget(block, area);
        let width = usize::from(inner.width);
        if self.wrapped.width != width {
            self.wrapped.width = width;
            self.wrapped.invalidate();
            self.wrapped.clear_goal();
        }
        self.wrapped.height = usize::from(inner.height);
        if inner.is_empty() {
            return;
        }
        let cursor = self.text_area.cursor();
        let layout = self.wrapped.layout(&self.text_area);
        let (cursor_row, cursor_col) = layout.source_to_visual(cursor);
        let count = layout.row_count();
        let height = self.wrapped.height;
        self.wrapped.top = self.wrapped.top.min(count.saturating_sub(height));
        if cursor_row < self.wrapped.top {
            self.wrapped.top = cursor_row;
        } else if cursor_row >= self.wrapped.top + height {
            self.wrapped.top = cursor_row + 1 - height;
        }
        let top = self.wrapped.top;
        let layout = self.wrapped.layout.as_ref().unwrap();
        let selected = self.text_area.selection_range();
        let selection_style = Style::from(estyles.selection_style);
        let buf = frame.buffer_mut();
        buf.set_style(inner, Style::reset());
        for (offset, row) in layout.rows.iter().skip(top).take(height).enumerate() {
            let y = inner.y + offset as u16;
            for glyph in &row.glyphs {
                let is_selected = selected.is_some_and(|(start, end)| {
                    (row.source_row, glyph.source_start) < end
                        && (row.source_row, glyph.source_end) > start
                });
                let style = if is_selected {
                    selection_style
                } else {
                    Style::reset()
                };
                buf.set_stringn(
                    inner.x + glyph.column as u16,
                    y,
                    &glyph.text,
                    glyph.width,
                    style,
                );
            }
            let end = (row.source_row, row.end_col);
            if row.end_col == self.text_area.lines()[row.source_row].chars().count()
                && selected.is_some_and(|(start, stop)| start <= end && end < stop)
            {
                let x = row.glyphs.last().map_or(0, |g| g.column + g.width);
                if x < width {
                    buf[(inner.x + x as u16, y)].set_style(selection_style);
                }
            }
        }
        if self.is_active && cursor_col < width && cursor_row >= top && cursor_row < top + height {
            let style = Style::from(match self.mode {
                EditorMode::Normal => estyles.cursor_normal,
                EditorMode::Insert => estyles.cursor_insert,
                EditorMode::Visual => estyles.cursor_visual,
            });
            let row = &layout.rows[cursor_row];
            let cursor_width = row
                .glyphs
                .iter()
                .find(|g| g.column == cursor_col)
                .map_or(1, |g| g.width);
            for x in cursor_col..(cursor_col + cursor_width).min(width) {
                buf[(inner.x + x as u16, inner.y + (cursor_row - top) as u16)].set_style(style);
            }
        }
        if count > height {
            let mut state = ScrollbarState::new(count.saturating_sub(height) + 1)
                .position(top)
                .viewport_content_length(height);
            frame.render_stateful_widget(
                Scrollbar::new(ScrollbarOrientation::VerticalRight)
                    .begin_symbol(Some("▲"))
                    .end_symbol(Some("▼"))
                    .track_symbol(Some(symbols::line::VERTICAL))
                    .thumb_symbol(symbols::block::FULL),
                area.inner(Margin {
                    horizontal: 0,
                    vertical: 1,
                }),
                &mut state,
            );
        }
    }
}

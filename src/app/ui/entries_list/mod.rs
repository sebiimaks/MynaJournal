use chrono::Datelike;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    prelude::Margin,
    style::Style,
    symbols,
    text::{Line, Span},
    widgets::{
        Block, Borders, List, ListItem, ListState, Paragraph, Scrollbar, ScrollbarOrientation,
        ScrollbarState, Wrap,
    },
};

use backend::DataProvider;

use crate::app::App;
use crate::{app::keymap::Keymap, settings::DatumVisibility};

use super::{Styles, UICommand, ui_functions::menu_key_style};

const LIST_INNER_MARGIN: usize = 5;
const MAX_TITLE_LINES: usize = 4;
const MAX_TAG_LINES: usize = 3;

#[derive(Debug)]
pub struct EntriesList {
    pub state: ListState,
    is_active: bool,
    pub multi_select_mode: bool,
}

impl EntriesList {
    pub fn new() -> Self {
        Self {
            state: ListState::default(),
            is_active: false,
            multi_select_mode: false,
        }
    }

    fn render_list<D: DataProvider>(
        &mut self,
        frame: &mut Frame,
        app: &App<D>,
        area: Rect,
        styles: &Styles,
    ) {
        let jstyles = &styles.journals_list;
        let width = usize::from(area.width)
            .saturating_sub(LIST_INNER_MARGIN)
            .max(1);
        let title_height = usize::from(area.height)
            .saturating_sub(2)
            .clamp(1, MAX_TITLE_LINES);

        let mut lines_count = 0;

        let items: Vec<ListItem> = app
            .get_active_entries()
            .map(|entry| {
                let highlight_selected =
                    self.multi_select_mode && app.selected_entries.contains(&entry.id);

                // *** Title ***
                let (mut title, title_truncated) =
                    bounded_preview(&entry.title, width * title_height);
                if entry.protection.is_some() {
                    title.insert_str(
                        0,
                        if app.is_locked(entry.id) {
                            "[Locked] "
                        } else {
                            "[Open] "
                        },
                    );
                }

                if highlight_selected {
                    title.insert_str(0, "* ");
                }

                // Text wrapping
                let title_lines = preview_lines(&title, width, title_height, title_truncated);

                // tilte lines
                lines_count += title_lines.len();

                let title_style = match (self.is_active, highlight_selected) {
                    (_, true) => jstyles.title_selected,
                    (true, _) => jstyles.title_active,
                    (false, _) => jstyles.title_inactive,
                };

                let mut spans: Vec<Line> = title_lines
                    .iter()
                    .map(|line| Line::from(Span::styled(line.to_string(), title_style)))
                    .collect();

                // *** Date & Priority ***
                let date_priority_lines = match (app.settings.datum_visibility, entry.priority) {
                    (DatumVisibility::Show, Some(prio)) => {
                        let one_liner = format!(
                            "{}.{}.{} | Priority: {}",
                            entry.date.day(),
                            entry.date.month(),
                            entry.date.year(),
                            prio
                        );

                        if one_liner.len() > width {
                            vec![
                                format!(
                                    "{}.{}.{}",
                                    entry.date.day(),
                                    entry.date.month(),
                                    entry.date.year()
                                ),
                                format!("Priority: {prio}"),
                            ]
                        } else {
                            vec![one_liner]
                        }
                    }
                    (DatumVisibility::Show, None) => {
                        vec![format!(
                            "{}.{}.{}",
                            entry.date.day(),
                            entry.date.month(),
                            entry.date.year()
                        )]
                    }
                    (DatumVisibility::Hide, None) => Vec::new(),
                    (DatumVisibility::EmptyLine, None) => vec![String::new()],
                    (_, Some(prio)) => {
                        vec![format!("Priority: {}", prio)]
                    }
                };

                let date_lines = date_priority_lines
                    .iter()
                    .map(|line| Line::from(Span::styled(line.to_string(), jstyles.date_priority)));
                spans.extend(date_lines);

                // date & priority lines
                lines_count += date_priority_lines.len();

                // *** Tags ***
                if !entry.tags.is_empty() {
                    const TAGS_SEPARATOR: &str = " | ";
                    let tags_default_style: Style = jstyles.tags_default.into();

                    let mut tag_lines = vec![Line::default()];
                    for tag in &entry.tags {
                        let (preview, truncated) = bounded_preview(tag, width);
                        let preview = if truncated || preview.width() > width {
                            ellipsize(&preview, width)
                        } else {
                            preview
                        };
                        let style = app
                            .get_color_for_tag(tag)
                            .map(|c| Style::default().bg(c.background).fg(c.foreground))
                            .unwrap_or(tags_default_style);
                        let last_line = tag_lines.last_mut().unwrap();
                        let separator = if last_line.spans.is_empty() {
                            ""
                        } else {
                            TAGS_SEPARATOR
                        };
                        if last_line.width() + separator.len() + preview.width() > width
                            && !last_line.spans.is_empty()
                        {
                            if tag_lines.len() == MAX_TAG_LINES {
                                let last = tag_lines.last_mut().unwrap();
                                let text = last
                                    .spans
                                    .iter()
                                    .map(|span| span.content.as_ref())
                                    .collect::<String>();
                                *last = Line::from(Span::styled(
                                    ellipsize(&text, width),
                                    tags_default_style,
                                ));
                                break;
                            }
                            tag_lines.push(Line::default());
                        }
                        let last_line = tag_lines.last_mut().unwrap();
                        if !last_line.spans.is_empty() {
                            last_line.push_span(Span::styled(TAGS_SEPARATOR, tags_default_style));
                        }
                        last_line.push_span(Span::styled(preview, style));
                    }
                    lines_count += tag_lines.len();
                    spans.extend(tag_lines);
                }

                ListItem::new(spans)
            })
            .collect();

        let items_count = items.len();

        let highlight_style = if self.is_active {
            jstyles.highlight_active
        } else {
            jstyles.highlight_inactive
        };

        let list = List::new(items)
            .block(self.get_list_block(app.filter.is_some(), Some(items_count), styles))
            .highlight_style(highlight_style)
            .highlight_symbol("> ");

        frame.render_stateful_widget(list, area, &mut self.state);

        if lines_count > usize::from(area.height).saturating_sub(2) && items_count > 0 {
            let avg_item_height = lines_count / items_count;

            self.render_scrollbar(
                frame,
                area,
                self.state.selected().unwrap_or(0),
                items_count,
                avg_item_height,
            );
        }
    }

    fn render_scrollbar(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        pos: usize,
        items_count: usize,
        avg_item_height: usize,
    ) {
        const VIEWPORT_ADJUST: usize = 4;
        let viewport_len =
            (usize::from(area.height) / avg_item_height.max(1)).saturating_sub(VIEWPORT_ADJUST);

        let mut state = ScrollbarState::default()
            .content_length(items_count)
            .viewport_content_length(viewport_len)
            .position(pos);

        let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
            .begin_symbol(Some("▲"))
            .end_symbol(Some("▼"))
            .track_symbol(Some(symbols::line::VERTICAL))
            .thumb_symbol(symbols::block::FULL);

        let scroll_area = area.inner(Margin {
            horizontal: 0,
            vertical: 1,
        });

        frame.render_stateful_widget(scrollbar, scroll_area, &mut state);
    }

    fn render_place_holder(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        list_keymaps: &[Keymap],
        has_filter: bool,
        styles: &Styles,
    ) {
        let place_holder_text = if self.multi_select_mode {
            Line::raw("No entries to select")
        } else {
            let mut spans = vec![Span::raw("Use ")];
            for (index, keymap) in list_keymaps
                .iter()
                .filter(|keymap| keymap.command == UICommand::CreateEntry)
                .enumerate()
            {
                if index > 0 {
                    spans.push(Span::raw(", "));
                }
                spans.push(Span::styled(keymap.key.to_string(), menu_key_style()));
            }
            spans.push(Span::raw(" to create new journal entry"));
            Line::from(spans)
        };

        let place_holder = Paragraph::new(vec![Line::default(), place_holder_text])
            .wrap(Wrap { trim: false })
            .alignment(Alignment::Center)
            .block(self.get_list_block(has_filter, None, styles));

        frame.render_widget(place_holder, area);
    }

    fn get_list_block<'a>(
        &self,
        has_filter: bool,
        entries_len: Option<usize>,
        styles: &Styles,
    ) -> Block<'a> {
        let title = match (self.multi_select_mode, has_filter) {
            (true, true) => "Entries - Multi-Select - Filtered",
            (true, false) => "Entries - Multi-Select",
            (false, true) => "Entries - Filtered",
            (false, false) => "Entries",
        };

        let border_style = match (self.is_active, self.multi_select_mode) {
            (_, true) => styles.journals_list.block_multi_select,
            (true, _) => styles.journals_list.block_active,
            (false, _) => styles.journals_list.block_inactive,
        };

        let block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .border_style(border_style);

        match (entries_len, self.state.selected().map(|v| v + 1)) {
            (Some(entries_len), Some(selected)) => {
                block.title_bottom(Line::from(format!("{selected}/{entries_len}")).right_aligned())
            }
            _ => block,
        }
    }

    pub fn render_widget<D: DataProvider>(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        app: &App<D>,
        list_keymaps: &[Keymap],
        styles: &Styles,
    ) {
        if app.get_active_entries().next().is_none() {
            self.render_place_holder(frame, area, list_keymaps, app.filter.is_some(), styles);
        } else {
            self.render_list(frame, app, area, styles);
        }
    }

    pub fn set_active(&mut self, active: bool) {
        self.is_active = active;
    }
}

fn bounded_preview(text: &str, limit: usize) -> (String, bool) {
    let mut chars = text.chars();
    let preview = chars
        .by_ref()
        .take(limit)
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    (preview, chars.next().is_some())
}

fn ellipsize(text: &str, width: usize) -> String {
    let mut remaining = width.saturating_sub(1);
    let mut result = String::new();
    for glyph in text.graphemes(true) {
        let cells = glyph.width();
        if cells > remaining {
            break;
        }
        result.push_str(glyph);
        remaining -= cells;
    }
    result.push('…');
    result
}

fn preview_lines(text: &str, width: usize, height: usize, already_truncated: bool) -> Vec<String> {
    let wrapped = textwrap::wrap(text, width);
    let truncated = already_truncated || wrapped.len() > height;
    let mut lines: Vec<String> = wrapped
        .into_iter()
        .take(height)
        .map(|line| line.into_owned())
        .collect();
    if truncated && let Some(last) = lines.last_mut() {
        *last = ellipsize(last, width);
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::test::create_default_app;
    use ratatui::{Terminal, backend::TestBackend};

    #[tokio::test]
    async fn oversized_titles_and_tags_render_without_mutating_stored_text() {
        let mut app = create_default_app();
        app.load_entries().await.unwrap();
        app.entries[0].title = "x".repeat(1_638_375);
        app.entries[0].tags = vec!["中".repeat(100_000); 10];
        let mut list = EntriesList::new();
        list.state.select(Some(0));
        let mut terminal = Terminal::new(TestBackend::new(30, 30)).unwrap();
        terminal
            .draw(|frame| list.render_widget(frame, frame.area(), &app, &[], &Styles::default()))
            .unwrap();
        let screen = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(screen.contains('…'));
        assert_eq!(app.entries[0].title.len(), 1_638_375);
        assert_eq!(app.entries[0].tags[0].chars().count(), 100_000);
    }

    #[tokio::test]
    async fn narrow_and_empty_viewports_do_not_underflow() {
        let mut app = create_default_app();
        app.load_entries().await.unwrap();
        let mut list = EntriesList::new();
        for width in 0..=6 {
            for height in 0..=3 {
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal
                    .draw(|frame| {
                        list.render_widget(frame, frame.area(), &app, &[], &Styles::default())
                    })
                    .unwrap();
            }
        }
    }

    #[test]
    fn preview_truncation_keeps_unicode_and_normal_titles_readable() {
        let (text, truncated) = bounded_preview("hello world", 80);
        assert_eq!(preview_lines(&text, 12, 4, truncated), ["hello world"]);
        let (text, truncated) = bounded_preview("中".repeat(100).as_str(), 12);
        let lines = preview_lines(&text, 4, 3, truncated);
        assert_eq!(lines.len(), 3);
        assert!(lines.last().unwrap().ends_with('…'));
        assert!(lines.iter().all(|line| line.width() <= 4));
    }
}

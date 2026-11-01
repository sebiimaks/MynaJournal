use anyhow::Ok;
use chrono::{
    DateTime, Datelike, Local, LocalResult, NaiveDate, NaiveDateTime, NaiveTime, TimeZone,
    Timelike, Utc,
};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use tui_textarea::{CursorMove, TextArea};

use crate::{
    app::{App, keymap::Input},
    settings::{EntryTimeFormat, Settings},
};

use backend::{DataProvider, Entry, Mood};

use self::tags::{TagsPopup, TagsPopupReturn};

use super::{
    Styles,
    ui_functions::{centered_rect_exact_dimensions, centered_rect_exact_height, menu_key_style},
};

mod snapshot;
mod tags;
#[cfg(test)]
mod tests;

const HORIZONTAL_MARGIN: u16 = 4;

pub struct EntryPopup<'a> {
    title_txt: TextArea<'a>,
    date_txt: TextArea<'a>,
    tags_txt: TextArea<'a>,
    priority_txt: TextArea<'a>,
    show_priority: bool,
    show_mood: bool,
    mood: Option<Mood>,
    mood_focus: usize,
    time_format: Option<EntryTimeFormat>,
    is_edit_entry: bool,
    original_date: Option<DateTime<Utc>>,
    active_txt: ActiveText,
    title_err_msg: String,
    date_err_msg: String,
    tags_err_msg: String,
    priority_err_msg: String,
    tags_popup: Option<TagsPopup>,
}

#[derive(Debug, PartialEq, Eq)]
enum ActiveText {
    Title,
    Mood,
    Date,
    Tags,
    Priority,
}

#[derive(Debug, PartialEq, Eq)]
pub enum EntryPopupInputReturn {
    KeepPopup,
    Cancel,
    AddEntry(u32),
    UpdateCurrentEntry,
}

impl EntryPopup<'_> {
    pub fn new_entry(settings: &Settings) -> Self {
        let mut title_txt = if let Some(auto_title) = &settings.auto_title {
            TextArea::new(vec![auto_title.resolve()])
        } else {
            TextArea::default()
        };
        title_txt.move_cursor(CursorMove::End);

        let date = Local::now();
        let time_format = settings
            .show_entry_time
            .then_some(settings.entry_time_format);
        let date_txt = TextArea::new(vec![date_input_text(date, time_format)]);

        let tags_txt = TextArea::default();

        let priority_txt = if let Some(priority) = settings.default_journal_priority {
            TextArea::new(vec![priority.to_string()])
        } else {
            TextArea::default()
        };

        Self {
            title_txt,
            date_txt,
            tags_txt,
            priority_txt,
            show_priority: settings.show_entry_priority,
            show_mood: settings.show_entry_mood,
            mood: None,
            mood_focus: 0,
            time_format,
            is_edit_entry: false,
            original_date: Some(date.with_timezone(&Utc)),
            active_txt: ActiveText::Title,
            title_err_msg: String::default(),
            date_err_msg: String::default(),
            tags_err_msg: String::default(),
            priority_err_msg: String::default(),
            tags_popup: None,
        }
    }

    pub fn from_entry(entry: &Entry, settings: &Settings) -> Self {
        let mut title_txt = TextArea::new(vec![entry.title.to_owned()]);
        title_txt.move_cursor(CursorMove::End);

        let date = entry.date.with_timezone(&Local);
        let time_format = settings
            .show_entry_time
            .then_some(settings.entry_time_format);
        let date_txt = TextArea::new(vec![date_input_text(date, time_format)]);

        let tags = tags_to_text(&entry.tags);

        let mut tags_txt = TextArea::new(vec![tags]);
        tags_txt.move_cursor(CursorMove::End);

        let prio = entry.priority.map(|pr| pr.to_string()).unwrap_or_default();

        let mut priority_txt = TextArea::new(vec![prio]);
        priority_txt.move_cursor(CursorMove::End);

        let mut entry_popup = Self {
            title_txt,
            date_txt,
            tags_txt,
            priority_txt,
            show_priority: settings.show_entry_priority,
            show_mood: settings.show_entry_mood,
            mood: entry.mood,
            mood_focus: entry
                .mood
                .and_then(|m| Mood::ALL.iter().position(|candidate| *candidate == m))
                .unwrap_or(0),
            time_format,
            is_edit_entry: true,
            original_date: Some(entry.date),
            active_txt: ActiveText::Title,
            title_err_msg: String::default(),
            date_err_msg: String::default(),
            tags_err_msg: String::default(),
            priority_err_msg: String::default(),
            tags_popup: None,
        };

        entry_popup.validate_all();

        entry_popup
    }

    pub fn render_widget(&mut self, frame: &mut Frame, area: Rect, styles: &Styles) {
        let command_style = menu_key_style();
        let footer_text = Line::from(vec![
            Span::raw("Confirm: "),
            Span::styled("Enter", command_style),
            Span::raw(", "),
            Span::styled("<Ctrl-m>", command_style),
            Span::raw(" | Cancel: "),
            Span::styled("Esc", command_style),
            Span::raw(", "),
            Span::styled("<Ctrl-c>", command_style),
            Span::raw(" | Change focus: "),
            Span::styled("Tab", command_style),
            Span::raw(" | Open Tags: "),
            Span::styled("<Ctrl-Space>", command_style),
            Span::raw(", "),
            Span::styled("<Ctrl-t>", command_style),
        ]);
        let mut footer = Paragraph::new(footer_text)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: false });
        let bounds = area;
        let height: u16 = if self.show_priority { 17 } else { 14 };
        let width = centered_rect_exact_height(70, height, bounds).width;
        let width = if self.is_edit_entry {
            width
        } else {
            (u32::from(width) * 27 / 40) as u16
        };
        let width = if self.show_mood {
            let minimum_width = if self.is_edit_entry { 42 } else { 38 };
            width.max(minimum_width).min(bounds.width)
        } else {
            width
        };
        let horizontal_margin = if width < 42 { 1 } else { HORIZONTAL_MARGIN };
        let inner_width = width.saturating_sub(2 * horizontal_margin);
        let mood_lines = self.mood_lines(inner_width, styles);
        let mood_height = mood_lines.len() as u16;
        let height = height + if self.show_mood { mood_height } else { 0 };
        let mut mood_hint = vec![
            Span::raw("Press "),
            Span::styled("Space", command_style),
            Span::raw(" to select"),
        ];
        if inner_width >= 35 {
            mood_hint.extend([
                Span::raw(" | "),
                Span::styled("←/→", command_style),
                Span::raw(" to move"),
            ]);
        }
        let mood_hint = Paragraph::new(Line::from(mood_hint))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: false });
        let hint_height = if self.show_mood {
            mood_hint.line_count(inner_width).min(u16::MAX as usize) as u16
        } else {
            0
        };
        let mut footer_height = (footer.line_count(inner_width).min(u16::MAX as usize) as u16)
            .saturating_add(hint_height);
        let mut height = height.saturating_add(footer_height.saturating_sub(1));
        let vertical_margin = if height > bounds.height { 1 } else { 2 };
        if height > bounds.height {
            let compact = vec![
                Span::styled("Enter", command_style),
                Span::raw(": confirm | "),
                Span::styled("Esc", command_style),
                Span::raw(": cancel | "),
                Span::styled("Tab", command_style),
                Span::raw(": next"),
            ];
            footer = Paragraph::new(Line::from(compact))
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: false });
            footer_height = (footer.line_count(inner_width).min(u16::MAX as usize) as u16)
                .saturating_add(hint_height);
            height = bounds.height;
        }
        let area = centered_rect_exact_dimensions(width, height, bounds).intersection(bounds);

        let block = Block::default()
            .borders(Borders::ALL)
            .title(if self.is_edit_entry {
                "Edit journal"
            } else {
                "Create Entry"
            });

        frame.render_widget(Clear, area);
        frame.render_widget(block, area);

        let inner = area.inner(ratatui::layout::Margin::new(
            horizontal_margin,
            vertical_margin,
        ));
        let footer_height = footer_height.min(inner.height.saturating_sub(3));
        let body_height = inner.height.saturating_sub(footer_height);
        if inner.width < 3 || body_height < 3 {
            frame.render_widget(Paragraph::new("Enlarge terminal"), inner);
            return;
        }
        let mut fields = vec![(0, ActiveText::Title, 3)];
        if self.show_mood {
            fields.push((1, ActiveText::Mood, mood_height));
        }
        fields.push((2, ActiveText::Date, 3));
        if self.show_priority {
            fields.push((3, ActiveText::Priority, 3));
        }
        fields.push((4, ActiveText::Tags, 3));
        let focus = fields
            .iter()
            .position(|(_, field, _)| *field == self.active_txt)
            .unwrap_or(0);
        if fields[focus].2 > body_height {
            frame.render_widget(Paragraph::new("Enlarge terminal"), inner);
            return;
        }
        let mut start = 0;
        while start < focus
            && fields[start..=focus]
                .iter()
                .map(|(_, _, height)| height)
                .sum::<u16>()
                > body_height
        {
            start += 1;
        }
        let mut chunks = [Rect::default(); 6];
        let mut y = inner.y;
        for (index, _, height) in &fields[start..] {
            if y + height > inner.y + body_height {
                break;
            }
            chunks[*index] = Rect::new(inner.x, y, inner.width, *height);
            y += height;
        }
        chunks[5] = Rect::new(
            inner.x,
            inner.bottom() - footer_height,
            inner.width,
            footer_height,
        );

        if self.show_mood {
            frame.render_widget(Paragraph::new(mood_lines), chunks[1]);
        }
        self.title_txt.set_cursor_line_style(Style::default());
        self.date_txt.set_cursor_line_style(Style::default());
        self.tags_txt.set_cursor_line_style(Style::default());
        self.priority_txt.set_cursor_line_style(Style::default());

        let gstyles = &styles.general;

        let active_block_style = Style::from(gstyles.input_block_active);
        let reset_style = Style::reset();
        let invalid_block_style = Style::from(gstyles.input_block_invalid);

        let active_cursor_style = Style::from(gstyles.input_cursor_active);
        let deactivate_cursor_style = Style::default().bg(Color::Reset);
        let invalid_cursor_style = Style::from(gstyles.input_cursor_invalid);

        if self.title_err_msg.is_empty() {
            let (block, cursor) = match self.active_txt {
                ActiveText::Title => (active_block_style, active_cursor_style),
                _ => (reset_style, deactivate_cursor_style),
            };
            self.title_txt.set_style(block);
            self.title_txt.set_cursor_style(cursor);
            self.title_txt.set_block(
                Block::default()
                    .borders(Borders::ALL)
                    .style(block)
                    .title("Title"),
            );
        } else {
            let cursor = if self.active_txt == ActiveText::Title {
                invalid_cursor_style
            } else {
                deactivate_cursor_style
            };

            self.title_txt.set_style(invalid_block_style);
            self.title_txt.set_cursor_style(cursor);
            self.title_txt.set_block(
                Block::default()
                    .borders(Borders::ALL)
                    .style(invalid_block_style)
                    .title(format!("Title : {}", self.title_err_msg)),
            );
        }

        let date_label = if self.time_format.is_some() {
            "Date and Time"
        } else {
            "Date"
        };
        if self.date_err_msg.is_empty() {
            let (block, cursor) = match self.active_txt {
                ActiveText::Date => (active_block_style, active_cursor_style),
                _ => (reset_style, deactivate_cursor_style),
            };
            self.date_txt.set_style(block);
            self.date_txt.set_cursor_style(cursor);
            self.date_txt.set_block(
                Block::default()
                    .borders(Borders::ALL)
                    .style(block)
                    .title(date_label),
            );
        } else {
            let cursor = if self.active_txt == ActiveText::Date {
                invalid_cursor_style
            } else {
                deactivate_cursor_style
            };
            self.date_txt.set_style(invalid_block_style);
            self.date_txt.set_cursor_style(cursor);
            self.date_txt.set_block(
                Block::default()
                    .borders(Borders::ALL)
                    .style(invalid_block_style)
                    .title(format!("{date_label} : {}", self.date_err_msg)),
            );
        }

        if self.tags_err_msg.is_empty() {
            let (block, cursor, title) = match self.active_txt {
                ActiveText::Tags => (
                    active_block_style,
                    active_cursor_style,
                    "Tags - A comma-separated list",
                ),
                _ => (reset_style, deactivate_cursor_style, "Tags"),
            };
            self.tags_txt.set_style(block);
            self.tags_txt.set_cursor_style(cursor);
            self.tags_txt.set_block(
                Block::default()
                    .borders(Borders::ALL)
                    .style(block)
                    .title(title),
            );
        } else {
            let cursor = if self.active_txt == ActiveText::Tags {
                invalid_cursor_style
            } else {
                deactivate_cursor_style
            };
            self.tags_txt.set_style(invalid_block_style);
            self.tags_txt.set_cursor_style(cursor);
            self.tags_txt.set_block(
                Block::default()
                    .borders(Borders::ALL)
                    .style(invalid_block_style)
                    .title(format!("Tags : {}", self.date_err_msg)),
            );
        }

        if self.priority_err_msg.is_empty() {
            let (block, cursor) = match self.active_txt {
                ActiveText::Priority => (active_block_style, active_cursor_style),
                _ => (reset_style, deactivate_cursor_style),
            };
            self.priority_txt.set_style(block);
            self.priority_txt.set_cursor_style(cursor);
            self.priority_txt.set_block(
                Block::default()
                    .borders(Borders::ALL)
                    .style(block)
                    .title("Priority"),
            );
        } else {
            let cursor = if self.active_txt == ActiveText::Priority {
                invalid_cursor_style
            } else {
                deactivate_cursor_style
            };
            self.priority_txt.set_style(invalid_block_style);
            self.priority_txt.set_cursor_style(cursor);
            self.priority_txt.set_block(
                Block::default()
                    .borders(Borders::ALL)
                    .style(invalid_block_style)
                    .title(format!("Priority : {}", self.priority_err_msg)),
            );
        }

        frame.render_widget(&self.title_txt, chunks[0]);
        frame.render_widget(&self.date_txt, chunks[2]);
        if self.show_priority {
            frame.render_widget(&self.priority_txt, chunks[3]);
        }
        frame.render_widget(&self.tags_txt, chunks[4]);

        let hint_height = hint_height.min(chunks[5].height);
        let footer_area = Rect::new(
            chunks[5].x,
            chunks[5].y,
            chunks[5].width,
            chunks[5].height - hint_height,
        );
        frame.render_widget(footer, footer_area);
        if self.show_mood && self.active_txt == ActiveText::Mood {
            frame.render_widget(
                mood_hint,
                Rect::new(
                    chunks[5].x,
                    footer_area.bottom(),
                    chunks[5].width,
                    hint_height,
                ),
            );
        }

        if let Some(tags_popup) = self.tags_popup.as_mut() {
            tags_popup.render_widget(frame, area, styles)
        }
    }

    fn mood_lines(&self, width: u16, styles: &Styles) -> Vec<Line<'static>> {
        let active = self.active_txt == ActiveText::Mood;
        let style = if active {
            Style::from(styles.general.input_block_active)
        } else {
            Style::reset()
        };
        let choices: Vec<_> = Mood::ALL
            .into_iter()
            .enumerate()
            .map(|(index, mood)| {
                let choice = format!(
                    "[{}] {}",
                    if self.mood == Some(mood) { 'x' } else { ' ' },
                    mood.label()
                );
                let choice_style = if active && self.mood_focus == index {
                    Style::from(styles.general.input_cursor_active)
                } else {
                    style
                };
                Span::styled(choice, choice_style)
            })
            .collect();
        let mut lines = vec![Line::default()];
        lines.extend(
            textwrap::wrap("How is your mood at the moment?", usize::from(width.max(1)))
                .into_iter()
                .map(|question| {
                    Line::from(Span::styled(question.into_owned(), style))
                        .alignment(Alignment::Center)
                }),
        );
        lines.push(Line::default());
        let mut row = Vec::new();
        for choice in choices {
            if !row.is_empty() {
                row.push(Span::raw(" "));
            }
            row.push(choice);
            if width < 28 {
                lines.push(Line::from(std::mem::take(&mut row)).alignment(Alignment::Center));
            }
        }
        if !row.is_empty() {
            lines.push(Line::from(row).alignment(Alignment::Center));
        }
        lines.push(Line::default());
        lines
    }

    pub fn is_input_valid(&self) -> bool {
        self.title_err_msg.is_empty()
            && self.date_err_msg.is_empty()
            && self.tags_err_msg.is_empty()
            && self.priority_err_msg.is_empty()
    }

    pub fn validate_all(&mut self) {
        self.validate_title();
        self.validate_date();
        self.validate_tags();
        self.validate_priority();
    }

    fn validate_title(&mut self) {
        if self.title_txt.lines()[0].is_empty() {
            self.title_err_msg = "Title can't be empty".into();
        } else {
            self.title_err_msg.clear();
        }
    }

    fn validate_date(&mut self) {
        if let Err(err) = self.parse_date() {
            self.date_err_msg = err.to_string();
        } else {
            self.date_err_msg.clear();
        }
    }

    fn parse_date(&self) -> Result<(NaiveDate, Option<NaiveTime>), chrono::ParseError> {
        let text = self.date_txt.lines()[0].as_str();
        match self.time_format {
            Some(format) => {
                let format = match format {
                    EntryTimeFormat::TwentyFourHour => "%d-%m-%Y %H:%M",
                    EntryTimeFormat::TwelveHour => "%d-%m-%Y %I:%M %P",
                };
                NaiveDateTime::parse_from_str(text, format)
                    .map(|value| (value.date(), Some(value.time())))
            }
            None => NaiveDate::parse_from_str(text, "%d-%m-%Y").map(|date| (date, None)),
        }
    }

    fn validate_tags(&mut self) {
        let tags = text_to_tags(
            self.tags_txt
                .lines()
                .first()
                .expect("Tags TextBox have one line"),
        );
        if tags.iter().any(|tag| tag.contains(',')) {
            self.tags_err_msg = "Tags are invalid".into();
        } else {
            self.tags_err_msg.clear();
        }
    }

    fn validate_priority(&mut self) {
        let prio_text = self.priority_txt.lines().first().unwrap();
        if !prio_text.is_empty() && prio_text.parse::<u32>().is_err() {
            self.priority_err_msg = String::from("Priority must be a positive number");
        } else {
            self.priority_err_msg.clear();
        }
    }

    pub async fn handle_input<D: DataProvider>(
        &mut self,
        input: &Input,
        app: &mut App<D>,
    ) -> anyhow::Result<EntryPopupInputReturn> {
        if self.tags_popup.is_some() {
            self.handle_tags_popup_input(input);

            return Ok(EntryPopupInputReturn::KeepPopup);
        }

        let has_ctrl = input.modifiers.contains(KeyModifiers::CONTROL);

        match input.key_code {
            KeyCode::Esc => Ok(EntryPopupInputReturn::Cancel),
            KeyCode::Char('c') if has_ctrl => Ok(EntryPopupInputReturn::Cancel),
            KeyCode::Enter => self.handle_confirm(app).await,
            KeyCode::Char('m') if has_ctrl => self.handle_confirm(app).await,
            KeyCode::Tab | KeyCode::Down => {
                self.active_txt = match self.active_txt {
                    ActiveText::Title if self.show_mood => ActiveText::Mood,
                    ActiveText::Title | ActiveText::Mood => ActiveText::Date,
                    ActiveText::Date if self.show_priority => ActiveText::Priority,
                    ActiveText::Date => ActiveText::Tags,
                    ActiveText::Priority => ActiveText::Tags,
                    ActiveText::Tags => ActiveText::Title,
                };
                Ok(EntryPopupInputReturn::KeepPopup)
            }
            KeyCode::Up | KeyCode::BackTab => {
                self.active_txt = match self.active_txt {
                    ActiveText::Title => ActiveText::Tags,
                    ActiveText::Date if self.show_mood => ActiveText::Mood,
                    ActiveText::Date | ActiveText::Mood => ActiveText::Title,
                    ActiveText::Priority => ActiveText::Date,
                    ActiveText::Tags if self.show_priority => ActiveText::Priority,
                    ActiveText::Tags => ActiveText::Date,
                };
                Ok(EntryPopupInputReturn::KeepPopup)
            }
            KeyCode::Char(' ') | KeyCode::Char('t') if has_ctrl => {
                debug_assert!(self.tags_popup.is_none());

                let tags = app.get_all_tags();
                let tags_text = self
                    .tags_txt
                    .lines()
                    .first()
                    .expect("Tags text box has one line");

                self.tags_popup = Some(TagsPopup::new(tags_text, tags));

                Ok(EntryPopupInputReturn::KeepPopup)
            }
            _ => {
                match self.active_txt {
                    ActiveText::Mood => match input.key_code {
                        KeyCode::Left => self.mood_focus = (self.mood_focus + 2) % 3,
                        KeyCode::Right => self.mood_focus = (self.mood_focus + 1) % 3,
                        KeyCode::Char(' ') if input.modifiers.is_empty() => {
                            let focused = Mood::ALL[self.mood_focus];
                            self.mood = if self.mood == Some(focused) {
                                None
                            } else {
                                Some(focused)
                            };
                        }
                        _ => {}
                    },
                    ActiveText::Title => {
                        if self.title_txt.input(KeyEvent::from(input)) {
                            self.validate_title();
                        }
                    }
                    ActiveText::Date => {
                        if self.date_txt.input(KeyEvent::from(input)) {
                            self.validate_date();
                        }
                    }
                    ActiveText::Tags => {
                        if self.tags_txt.input(KeyEvent::from(input)) {
                            self.validate_tags();
                        }
                    }
                    ActiveText::Priority => {
                        if self.priority_txt.input(KeyEvent::from(input)) {
                            self.validate_priority();
                        }
                    }
                }
                Ok(EntryPopupInputReturn::KeepPopup)
            }
        }
    }

    pub fn handle_tags_popup_input(&mut self, input: &Input) {
        let tags_popup = self
            .tags_popup
            .as_mut()
            .expect("Tags popup must be some at this point");

        match tags_popup.handle_input(input) {
            TagsPopupReturn::Keep => {}
            TagsPopupReturn::Cancel => self.tags_popup = None,
            TagsPopupReturn::Apply(tags_text) => {
                self.tags_txt = TextArea::new(vec![tags_text]);
                self.tags_txt.move_cursor(CursorMove::End);
                self.active_txt = ActiveText::Tags;
                self.tags_popup = None;
            }
        }
    }

    async fn handle_confirm<D: DataProvider>(
        &mut self,
        app: &mut App<D>,
    ) -> anyhow::Result<EntryPopupInputReturn> {
        // Validation
        self.validate_all();
        if !self.is_input_valid() {
            self.active_txt = if !self.title_err_msg.is_empty() {
                ActiveText::Title
            } else if !self.date_err_msg.is_empty() {
                ActiveText::Date
            } else if !self.priority_err_msg.is_empty() {
                ActiveText::Priority
            } else {
                ActiveText::Tags
            };
            return Ok(EntryPopupInputReturn::KeepPopup);
        }

        let title = self.title_txt.lines()[0].to_owned();
        let (date, time) = self.parse_date().expect("Date must be valid here");

        let date = if let Some(original) = self.original_date.filter(|original| {
            let local = original.with_timezone(&Local);
            (local.year(), local.month(), local.day()) == (date.year(), date.month(), date.day())
                && time.is_none_or(|time| {
                    (local.hour(), local.minute()) == (time.hour(), time.minute())
                })
        }) {
            original
        } else {
            let resolved = match time {
                Some(time) => local_instant(date.and_time(time)),
                None => local_day_start(date),
            };
            let Some(resolved) = resolved else {
                self.date_err_msg = if time.is_some() {
                    "This time does not exist in your local timezone"
                } else {
                    "This date does not exist in your local timezone"
                }
                .into();
                self.active_txt = ActiveText::Date;
                return Ok(EntryPopupInputReturn::KeepPopup);
            };
            resolved
        };

        let tags = text_to_tags(
            self.tags_txt
                .lines()
                .first()
                .expect("Tags TextBox have one line"),
        );

        let priority = match self.priority_txt.lines().first().unwrap() {
            num if num.is_empty() => None,
            num => Some(num.parse().expect("Priority must be validated before")),
        };

        if self.is_edit_entry {
            app.update_current_entry_attributes(title, date, tags, priority, self.mood)
                .await?;
            Ok(EntryPopupInputReturn::UpdateCurrentEntry)
        } else {
            let entry_id = app
                .add_entry(title, date, tags, priority, self.mood)
                .await?;
            Ok(EntryPopupInputReturn::AddEntry(entry_id))
        }
    }
}

fn date_input_text(date: DateTime<Local>, time_format: Option<EntryTimeFormat>) -> String {
    let mut text = format!("{:02}-{:02}-{}", date.day(), date.month(), date.year());
    if let Some(format) = time_format {
        text.push_str(
            &date
                .format(match format {
                    EntryTimeFormat::TwentyFourHour => " %H:%M",
                    EntryTimeFormat::TwelveHour => " %I:%M %P",
                })
                .to_string(),
        );
    }
    text
}

fn local_instant(time: NaiveDateTime) -> Option<DateTime<Utc>> {
    let candidates = match Local.from_local_datetime(&time) {
        LocalResult::Single(value) => [Some(value), None],
        LocalResult::Ambiguous(first, second) => [Some(first), Some(second)],
        LocalResult::None => [None, None],
    };
    // Verify transition-boundary offsets and order ambiguous results by their instant.
    candidates
        .into_iter()
        .flatten()
        .filter(|value| value.with_timezone(&Local).offset() == value.offset())
        .min()
        .map(|value| value.with_timezone(&Utc))
}

fn local_day_start(date: NaiveDate) -> Option<DateTime<Utc>> {
    // Clock changes can skip midnight, including historical changes of less than a minute.
    (0..86_400).find_map(|second| {
        let time = date
            .and_hms_opt(second / 3_600, second % 3_600 / 60, second % 60)
            .expect("Valid time within a day");
        local_instant(time)
    })
}

fn tags_to_text(tags: &[String]) -> String {
    tags.join(", ")
}

fn text_to_tags(text: &str) -> Vec<String> {
    text.split_terminator(',')
        .map(|tag| String::from(tag.trim()))
        .collect()
}

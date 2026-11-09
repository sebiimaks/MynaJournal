use crossterm::event::{Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind};
use ratatui::{
    Frame,
    layout::{Alignment, Margin, Rect},
    style::Style,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
};
use tui_textarea::TextArea;
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use super::{Styles, ui_functions::menu_key_style};

const WORDMARK: &str = include_str!("../../../assets/myna_wordmark.txt");
const MAX_NAME_CHARS: usize = 80;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JournalChoice {
    pub id: String,
    pub name: String,
    pub is_personal: bool,
    pub locked: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub enum SelectorAction {
    None,
    Open(String),
    Create(String),
    Delete(String),
    Quit,
}

enum Screen {
    Browse,
    Create(Box<TextArea<'static>>),
    Delete { journal: JournalChoice, yes: bool },
}

pub struct JournalSelector {
    journals: Vec<JournalChoice>,
    selection: ListState,
    screen: Screen,
    message: Option<String>,
    is_error: bool,
    page_size: usize,
}

impl JournalSelector {
    pub fn new(journals: Vec<JournalChoice>) -> Self {
        let selection = ListState::default().with_selected((!journals.is_empty()).then_some(0));
        Self {
            journals,
            selection,
            screen: Screen::Browse,
            message: None,
            is_error: false,
            page_size: 1,
        }
    }

    pub fn replace_journals(&mut self, journals: Vec<JournalChoice>, preferred_id: Option<&str>) {
        let selected_id = preferred_id.map(str::to_owned).or_else(|| {
            self.selection
                .selected()
                .and_then(|index| self.journals.get(index))
                .map(|journal| journal.id.clone())
        });
        let old_index = self.selection.selected().unwrap_or(0);
        let selected = selected_id
            .and_then(|id| journals.iter().position(|journal| journal.id == id))
            .or_else(|| (!journals.is_empty()).then(|| old_index.min(journals.len() - 1)));
        self.journals = journals;
        self.selection.select(selected);
        self.screen = Screen::Browse;
        self.message = None;
    }

    pub fn set_error(&mut self, error: String) {
        self.message = Some(error);
        self.is_error = true;
    }

    pub fn set_notice(&mut self, notice: String) {
        self.message = Some(notice);
        self.is_error = false;
    }

    pub fn handle_event(&mut self, event: Event) -> SelectorAction {
        match event {
            Event::Paste(text) => {
                if let Screen::Create(input) = &mut self.screen {
                    if text.chars().any(char::is_control) {
                        self.set_error("Journal names must stay on one line.".into());
                    } else {
                        let mut edited = input.as_ref().clone();
                        edited.insert_str(text);
                        if edited.lines()[0].chars().count() <= MAX_NAME_CHARS {
                            **input = edited;
                            self.message = None;
                        } else {
                            self.set_error(
                                "Journal names can contain at most 80 characters.".into(),
                            );
                        }
                    }
                }
                SelectorAction::None
            }
            Event::Key(key) if key.kind == KeyEventKind::Press => self.handle_key(key),
            Event::Mouse(mouse) if matches!(self.screen, Screen::Browse) => {
                match mouse.kind {
                    MouseEventKind::ScrollUp => self.move_selection(-1),
                    MouseEventKind::ScrollDown => self.move_selection(1),
                    _ => {}
                }
                SelectorAction::None
            }
            _ => SelectorAction::None,
        }
    }

    fn handle_key(&mut self, key: KeyEvent) -> SelectorAction {
        if key.modifiers.intersects(
            KeyModifiers::ALT | KeyModifiers::SUPER | KeyModifiers::HYPER | KeyModifiers::META,
        ) {
            return SelectorAction::None;
        }
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        let cancel = key.code == KeyCode::Esc || (control && key.code == KeyCode::Char('c'));
        if cancel {
            if matches!(self.screen, Screen::Browse) {
                return SelectorAction::Quit;
            }
            self.screen = Screen::Browse;
            self.message = None;
            return SelectorAction::None;
        }
        let confirm = key.code == KeyCode::Enter || (control && key.code == KeyCode::Char('m'));
        if control && !confirm {
            return SelectorAction::None;
        }
        match &mut self.screen {
            Screen::Browse => {
                if confirm {
                    return self
                        .selected_journal()
                        .map(|journal| SelectorAction::Open(journal.id.clone()))
                        .unwrap_or(SelectorAction::None);
                }
                match key.code {
                    KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
                    KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
                    KeyCode::PageUp => self.move_selection(-(self.page_size as isize)),
                    KeyCode::PageDown => self.move_selection(self.page_size as isize),
                    KeyCode::Home => self
                        .selection
                        .select((!self.journals.is_empty()).then_some(0)),
                    KeyCode::End => self.selection.select(self.journals.len().checked_sub(1)),
                    KeyCode::Char('n' | 'N') => {
                        self.screen = Screen::Create(Box::default());
                        self.message = None;
                    }
                    KeyCode::Char('d' | 'D') => {
                        if let Some(journal) = self.selected_journal().cloned() {
                            if journal.is_personal {
                                self.set_error(
                                    "Personal uses your existing storage and cannot be deleted here."
                                        .into(),
                                );
                            } else {
                                self.screen = Screen::Delete {
                                    journal,
                                    yes: false,
                                };
                                self.message = None;
                            }
                        }
                    }
                    KeyCode::Char('q' | 'Q') => return SelectorAction::Quit,
                    _ => {}
                }
            }
            Screen::Create(input) => {
                if confirm {
                    let name = input.lines()[0].trim();
                    if name.is_empty() {
                        self.set_error("Enter a journal name.".into());
                    } else {
                        return SelectorAction::Create(name.to_owned());
                    }
                } else {
                    match key.code {
                        KeyCode::Char(character) if !character.is_control() => {
                            let mut edited = input.as_ref().clone();
                            edited.input(key);
                            if edited.lines()[0].chars().count() <= MAX_NAME_CHARS {
                                **input = edited;
                                self.message = None;
                            } else {
                                self.set_error(
                                    "Journal names can contain at most 80 characters.".into(),
                                );
                            }
                        }
                        KeyCode::Left
                        | KeyCode::Right
                        | KeyCode::Home
                        | KeyCode::End
                        | KeyCode::Backspace
                        | KeyCode::Delete => {
                            input.input(key);
                            self.message = None;
                        }
                        _ => {}
                    }
                }
            }
            Screen::Delete { journal, yes } => {
                if confirm {
                    if *yes {
                        return SelectorAction::Delete(journal.id.clone());
                    }
                    self.screen = Screen::Browse;
                    self.message = None;
                } else {
                    match key.code {
                        KeyCode::Left | KeyCode::Right | KeyCode::Tab | KeyCode::BackTab => {
                            *yes = !*yes;
                        }
                        KeyCode::Char('y' | 'Y') => {
                            return SelectorAction::Delete(journal.id.clone());
                        }
                        KeyCode::Char('n' | 'N') => {
                            self.screen = Screen::Browse;
                            self.message = None;
                        }
                        _ => {}
                    }
                }
            }
        }
        SelectorAction::None
    }

    fn selected_journal(&self) -> Option<&JournalChoice> {
        self.selection
            .selected()
            .and_then(|index| self.journals.get(index))
    }

    fn move_selection(&mut self, offset: isize) {
        if self.journals.is_empty() {
            self.selection.select(None);
            return;
        }
        let selected = self.selection.selected().unwrap_or(0);
        let next = selected
            .saturating_add_signed(offset)
            .min(self.journals.len() - 1);
        self.selection.select(Some(next));
    }

    pub fn render(&mut self, frame: &mut Frame, area: Rect, styles: &Styles) {
        frame.render_widget(Clear, area);
        let margin = u16::from(area.width > 4 && area.height > 4);
        let bounds = area.inner(Margin::new(margin, margin));
        let footer = self.footer(bounds.width.saturating_sub(4));
        let width = (footer.width() as u16).saturating_add(4).min(bounds.width);
        let footer = Paragraph::new(footer).wrap(Wrap { trim: false });
        let footer_height = footer.line_count(width.saturating_sub(4)).min(5) as u16;
        let message_height = self
            .message
            .as_deref()
            .map(|message| {
                Paragraph::new(message)
                    .wrap(Wrap { trim: false })
                    .line_count(width.saturating_sub(4))
                    .min(3) as u16
            })
            .unwrap_or(0);
        let content_height = match self.screen {
            Screen::Browse => self.journals.len().clamp(3, 9) as u16,
            Screen::Create(_) => 4,
            Screen::Delete { .. } => 6,
        };
        let height = (content_height + footer_height + message_height + 4).min(bounds.height);
        let (dialog, wordmark) = selector_layout(bounds, width, height);
        if let Some((wordmark_area, text)) = wordmark {
            frame.render_widget(
                Paragraph::new(text).style(Style::from(styles.editor.block_normal_inactive)),
                wordmark_area,
            );
        }

        let block = Block::default()
            .borders(Borders::ALL)
            .title("Select Journal")
            .border_style(Style::from(styles.general.input_block_active));
        let inner = block.inner(dialog).inner(Margin::new(1, 0));
        frame.render_widget(block, dialog);
        if inner.width < 3 || inner.height < 3 {
            frame.render_widget(Paragraph::new("Enlarge terminal"), inner);
            return;
        }
        let minimum_content = match self.screen {
            Screen::Browse => 1,
            Screen::Create(_) => 3,
            Screen::Delete { .. } => 5,
        }
        .min(inner.height);
        let footer_height = footer_height.min(inner.height.saturating_sub(minimum_content));
        let gap = u16::from(inner.height > minimum_content + footer_height);
        let message_height = message_height.min(
            inner
                .height
                .saturating_sub(footer_height + minimum_content + gap),
        );
        let footer_area = Rect::new(
            inner.x,
            inner.bottom() - footer_height,
            inner.width,
            footer_height,
        );
        let message_area = Rect::new(
            inner.x,
            footer_area.y - message_height,
            inner.width,
            message_height,
        );
        let content = Rect::new(
            inner.x,
            inner.y,
            inner.width,
            message_area.y.saturating_sub(inner.y).saturating_sub(gap),
        );
        match &mut self.screen {
            Screen::Browse => {
                self.page_size = usize::from(content.height.max(1));
                if self.journals.is_empty() {
                    frame
                        .render_widget(Paragraph::new("No journals. Press n to add one."), content);
                } else {
                    let items: Vec<_> = self
                        .journals
                        .iter()
                        .map(|journal| {
                            let width = usize::from(content.width.saturating_sub(2));
                            let label = if journal.locked { "Locked" } else { "" };
                            let label = clipped_name(label, width);
                            let name_width =
                                width.saturating_sub(label.width() + usize::from(journal.locked));
                            let name = clipped_name(&journal.name, name_width);
                            let padding = width.saturating_sub(name.width() + label.width());
                            ListItem::new(format!("{name}{}{label}", " ".repeat(padding)))
                        })
                        .collect();
                    let list = List::new(items)
                        .highlight_symbol("> ")
                        .highlight_style(Style::from(styles.general.list_highlight_active));
                    frame.render_stateful_widget(list, content, &mut self.selection);
                }
            }
            Screen::Create(input) => {
                input.set_cursor_line_style(Style::default());
                input.set_cursor_style(Style::from(styles.general.input_cursor_active));
                input.set_style(Style::reset());
                input.set_block(
                    Block::default()
                        .title("Journal name")
                        .borders(if content.height >= 3 {
                            Borders::ALL
                        } else {
                            Borders::NONE
                        })
                        .border_style(Style::from(styles.general.input_block_active)),
                );
                let input_area =
                    Rect::new(content.x, content.y, content.width, content.height.min(3));
                frame.render_widget(input.as_ref(), input_area);
            }
            Screen::Delete { journal, yes } => {
                let warning = Paragraph::new(format!(
                    "Delete “{}”?\nMove to Deleted Journals.",
                    clipped_name(&journal.name, usize::from(content.width.saturating_sub(10)))
                ))
                .wrap(Wrap { trim: false });
                let warning_area = Rect::new(
                    content.x,
                    content.y,
                    content.width,
                    content.height.saturating_sub(2),
                );
                frame.render_widget(warning, warning_area);
                let choices = [(true, "Yes"), (false, "No")]
                    .into_iter()
                    .map(|(answer, label)| {
                        Span::styled(
                            format!("[{}] {label}  ", if answer == *yes { 'x' } else { ' ' }),
                            if answer == *yes {
                                Style::from(styles.general.input_cursor_active)
                            } else {
                                Style::reset()
                            },
                        )
                    })
                    .collect::<Vec<_>>();
                if content.height > 0 {
                    frame.render_widget(
                        Paragraph::new(Line::from(choices)).alignment(Alignment::Center),
                        Rect::new(content.x, content.bottom() - 1, content.width, 1),
                    );
                }
            }
        }
        if let Some(message) = self.message.as_deref() {
            let message = Paragraph::new(message)
                .style(if self.is_error {
                    Style::from(styles.general.input_block_invalid)
                } else {
                    Style::reset()
                })
                .wrap(Wrap { trim: false });
            frame.render_widget(message, message_area);
        }
        frame.render_widget(footer, footer_area);
    }

    fn footer(&self, width: u16) -> Text<'static> {
        let commands: &[(&str, &str)] = match self.screen {
            Screen::Browse => &[
                ("Open", "Enter"),
                ("Add", "n"),
                ("Delete", "d"),
                ("Exit", "q"),
            ],
            Screen::Create(_) => &[("Create", "Enter"), ("Cancel", "Esc")],
            Screen::Delete { .. } => &[("Confirm", "Enter"), ("Cancel", "Esc"), ("Choose", "←/→")],
        };
        let mut lines = Vec::new();
        let mut row = Line::default();
        for (label, key) in commands.iter() {
            let command = vec![
                Span::raw(format!("{label}: ")),
                Span::styled(*key, menu_key_style()),
            ];
            let command_width = command.iter().map(Span::width).sum::<usize>();
            if !row.spans.is_empty() && row.width() + 3 + command_width > usize::from(width) {
                lines.push(std::mem::take(&mut row));
            }
            if !row.spans.is_empty() {
                row.push_span(Span::raw(" | "));
            }
            row.spans.extend(command);
        }
        lines.push(row);
        Text::from(lines)
    }
}

fn clipped_name(name: &str, width: usize) -> String {
    if name.width() <= width {
        return name.to_owned();
    }
    if width == 0 {
        return String::new();
    }
    let mut label = String::new();
    for grapheme in name.graphemes(true) {
        if label.width() + grapheme.width() >= width {
            break;
        }
        label.push_str(grapheme);
    }
    label.push('…');
    label
}

fn centered(bounds: Rect, width: u16, height: u16) -> Rect {
    Rect::new(
        bounds.x + bounds.width.saturating_sub(width) / 2,
        bounds.y + bounds.height.saturating_sub(height) / 2,
        width.min(bounds.width),
        height.min(bounds.height),
    )
}

fn selector_layout(bounds: Rect, width: u16, height: u16) -> (Rect, Option<(Rect, &'static str)>) {
    for wordmark in [WORDMARK, "MynaJournal"] {
        let text = Text::raw(wordmark);
        let text_width = text.width() as u16;
        let text_height = text.height() as u16;
        let group_height = height + text_height + 2;
        if bounds.width >= text_width && bounds.height >= group_height {
            let group = centered(bounds, bounds.width, group_height);
            return (
                Rect::new(
                    bounds.x + (bounds.width - width) / 2,
                    group.y + text_height + 2,
                    width,
                    height,
                ),
                Some((
                    Rect::new(
                        bounds.x + (bounds.width - text_width) / 2,
                        group.y,
                        text_width,
                        text_height,
                    ),
                    wordmark,
                )),
            );
        }
    }
    (centered(bounds, width, height), None)
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;

    fn journals() -> Vec<JournalChoice> {
        vec![
            JournalChoice {
                id: "personal".into(),
                name: "Personal".into(),
                is_personal: true,
                locked: false,
            },
            JournalChoice {
                id: "work-id".into(),
                name: "Work".into(),
                is_personal: false,
                locked: false,
            },
        ]
    }

    fn key(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, KeyModifiers::NONE))
    }

    fn draw(selector: &mut JournalSelector, width: u16, height: u16) -> Terminal<TestBackend> {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| selector.render(frame, frame.area(), &Styles::default()))
            .unwrap();
        terminal
    }

    fn screen(terminal: &Terminal<TestBackend>) -> String {
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn selection_opens_by_stable_id_and_survives_list_reordering() {
        let mut selector = JournalSelector::new(journals());
        assert_eq!(
            selector.handle_event(key(KeyCode::Enter)),
            SelectorAction::Open("personal".into())
        );
        selector.handle_event(key(KeyCode::Down));
        assert_eq!(
            selector.handle_event(key(KeyCode::Enter)),
            SelectorAction::Open("work-id".into())
        );
        let mut reordered = journals();
        reordered.reverse();
        selector.replace_journals(reordered, None);
        assert_eq!(
            selector.handle_event(key(KeyCode::Enter)),
            SelectorAction::Open("work-id".into())
        );
        selector.replace_journals(journals(), Some("personal"));
        assert_eq!(
            selector.handle_event(key(KeyCode::Enter)),
            SelectorAction::Open("personal".into())
        );
        selector.replace_journals(Vec::new(), None);
        selector.handle_event(key(KeyCode::Down));
        assert_eq!(
            selector.handle_event(key(KeyCode::Enter)),
            SelectorAction::None
        );
    }

    #[test]
    fn create_supports_unicode_editing_and_preserves_name_on_save_error() {
        let mut selector = JournalSelector::new(journals());
        selector.handle_event(key(KeyCode::Char('n')));
        selector.handle_event(Event::Paste("  思い出 🐦  ".into()));
        assert_eq!(
            selector.handle_event(key(KeyCode::Enter)),
            SelectorAction::Create("思い出 🐦".into())
        );
        selector.set_error("The journal could not be saved.".into());
        let output = screen(&draw(&mut selector, 80, 24));
        assert!(output.contains("The journal could not be"));
        assert!(output.contains("saved."));
        selector.handle_event(key(KeyCode::End));
        for _ in 0..3 {
            selector.handle_event(key(KeyCode::Backspace));
        }
        assert_eq!(
            selector.handle_event(key(KeyCode::Enter)),
            SelectorAction::Create("思い出".into())
        );
        selector.handle_event(key(KeyCode::Esc));
        assert_eq!(
            selector.handle_event(key(KeyCode::Enter)),
            SelectorAction::Open("personal".into())
        );
    }

    #[test]
    fn names_reject_control_pastes_and_bound_input_without_truncating_it() {
        let mut selector = JournalSelector::new(journals());
        selector.handle_event(key(KeyCode::Char('n')));
        selector.handle_event(Event::Paste("Work\nHome".into()));
        assert!(selector.message.as_ref().unwrap().contains("one line"));
        selector.handle_event(Event::Paste("日".repeat(MAX_NAME_CHARS)));
        selector.handle_event(key(KeyCode::Char('x')));
        assert!(selector.message.as_ref().unwrap().contains("80 characters"));
        assert_eq!(
            selector.handle_event(key(KeyCode::Enter)),
            SelectorAction::Create("日".repeat(MAX_NAME_CHARS))
        );
    }

    #[test]
    fn deleting_defaults_to_no_and_requires_explicit_confirmation() {
        let mut selector = JournalSelector::new(journals());
        selector.handle_event(key(KeyCode::Char('d')));
        assert!(
            selector
                .message
                .as_ref()
                .unwrap()
                .contains("cannot be deleted")
        );
        selector.handle_event(key(KeyCode::Down));
        selector.handle_event(key(KeyCode::Char('d')));
        let output = screen(&draw(&mut selector, 80, 24));
        assert!(output.contains("Move to Deleted Journals."));
        assert!(output.contains("[ ] Yes  [x] No"));
        assert_eq!(
            selector.handle_event(key(KeyCode::Enter)),
            SelectorAction::None
        );
        assert!(matches!(selector.screen, Screen::Browse));
        selector.handle_event(key(KeyCode::Char('d')));
        selector.handle_event(key(KeyCode::Right));
        assert_eq!(
            selector.handle_event(key(KeyCode::Enter)),
            SelectorAction::Delete("work-id".into())
        );
        selector.handle_event(key(KeyCode::Esc));
        selector.handle_event(key(KeyCode::Char('d')));
        assert_eq!(
            selector.handle_event(key(KeyCode::Char('y'))),
            SelectorAction::Delete("work-id".into())
        );
    }

    #[test]
    fn delete_destination_and_choices_remain_visible_with_a_long_wide_name() {
        for (width, height) in [(80, 24), (60, 18), (31, 12)] {
            let mut choices = journals();
            choices[1].name = "家族👨‍👩‍👧‍👦".repeat(10);
            let mut selector = JournalSelector::new(choices);
            selector.handle_event(key(KeyCode::Down));
            selector.handle_event(key(KeyCode::Char('d')));
            let output = screen(&draw(&mut selector, width, height));
            assert!(output.contains('…'));
            assert!(output.contains("Deleted Journals."), "{width}x{height}");
            assert!(output.contains("[ ] Yes  [x] No"), "{width}x{height}");
            assert!(output.contains("Confirm: Enter"));
            assert!(output.contains("Cancel: Esc"));
        }
    }

    #[test]
    fn short_windows_keep_name_input_visible_after_validation_errors() {
        for (width, height) in [(80, 12), (60, 10), (31, 12)] {
            let mut selector = JournalSelector::new(journals());
            selector.handle_event(key(KeyCode::Char('n')));
            selector.handle_event(Event::Paste("Travel".into()));
            selector.set_error(
                "A journal with this name already exists. Please choose another name.".into(),
            );
            let output = screen(&draw(&mut selector, width, height));
            assert!(output.contains("Travel"), "Name hidden at {width}x{height}");
            assert!(output.contains("Create: Enter"));
            assert!(output.contains("Cancel: Esc"));
            selector.handle_event(key(KeyCode::Backspace));
            assert_eq!(
                selector.handle_event(key(KeyCode::Enter)),
                SelectorAction::Create("Trave".into())
            );
        }
    }

    #[test]
    fn key_releases_repeats_and_modified_commands_do_not_trigger_actions() {
        let mut selector = JournalSelector::new(journals());
        for kind in [KeyEventKind::Release, KeyEventKind::Repeat] {
            let mut event = KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE);
            event.kind = kind;
            assert_eq!(
                selector.handle_event(Event::Key(event)),
                SelectorAction::None
            );
        }
        for modifiers in [
            KeyModifiers::ALT,
            KeyModifiers::CONTROL,
            KeyModifiers::SUPER,
        ] {
            assert_eq!(
                selector.handle_event(Event::Key(KeyEvent::new(KeyCode::Char('q'), modifiers))),
                SelectorAction::None
            );
        }
        assert_eq!(
            selector.handle_event(Event::Key(KeyEvent::new(
                KeyCode::Char('m'),
                KeyModifiers::CONTROL
            ))),
            SelectorAction::Open("personal".into())
        );
        selector.handle_event(key(KeyCode::Char('n')));
        selector.handle_event(Event::Key(KeyEvent::new(
            KeyCode::Char('c'),
            KeyModifiers::CONTROL,
        )));
        assert!(matches!(selector.screen, Screen::Browse));
        assert_eq!(
            selector.handle_event(key(KeyCode::Esc)),
            SelectorAction::Quit
        );
    }

    #[test]
    fn wordmark_and_selector_are_centred_in_a_vertical_group() {
        for (width, height) in [(200, 60), (100, 30), (80, 24), (65, 24)] {
            let mut selector = JournalSelector::new(journals());
            let terminal = draw(&mut selector, width, height);
            let rows: Vec<String> = terminal
                .backend()
                .buffer()
                .content()
                .chunks(usize::from(width))
                .map(|row| row.iter().map(|cell| cell.symbol()).collect())
                .collect();
            let title_y = rows
                .iter()
                .position(|row| row.contains("Select Journal"))
                .unwrap();
            let first_line = WORDMARK.lines().next().unwrap();
            let (wordmark_y, wordmark_x) = rows
                .iter()
                .enumerate()
                .find_map(|(y, row)| {
                    row.find(first_line.trim())
                        .map(|x| (y, x - (first_line.len() - first_line.trim_start().len())))
                })
                .unwrap();
            for (offset, line) in WORDMARK.lines().enumerate() {
                assert_eq!(
                    &rows[wordmark_y + offset][wordmark_x..wordmark_x + line.len()],
                    line
                );
            }
            let wordmark_width = Text::raw(WORDMARK).width();
            assert!((2 * wordmark_x + wordmark_width).abs_diff(usize::from(width)) <= 1);
            assert_eq!(title_y, wordmark_y + WORDMARK.lines().count() + 2);
            let controls = "Open: Enter | Add: n | Delete: d | Exit: q";
            let left = rows[title_y].chars().position(|cell| cell == '┌').unwrap();
            let right = rows[title_y].chars().position(|cell| cell == '┐').unwrap();
            assert_eq!(right - left + 1, controls.width() + 4);
            assert!((left + right + 1).abs_diff(usize::from(width)) <= 1);
            let controls_row = rows.iter().find(|row| row.contains(controls)).unwrap();
            assert_eq!(
                controls_row
                    .chars()
                    .skip(left + 1)
                    .take(right - left - 1)
                    .collect::<String>(),
                format!(" {controls} ")
            );
            let bottom = rows
                .iter()
                .position(|row| row.chars().nth(left) == Some('└'))
                .unwrap();
            assert!((wordmark_y + bottom + 1).abs_diff(usize::from(height)) <= 1);
            assert!(rows.iter().any(|row| row.contains("Personal")));
            assert!(!screen(&terminal).contains("rrc%JkkZZ"));
        }
    }

    #[test]
    fn locked_journal_label_is_right_aligned_and_shares_selection_highlight() {
        for width in [31, 60, 100, 200] {
            let mut choices = journals();
            choices[0].locked = true;
            choices[0].name = "私の日記e\u{301}".repeat(20);
            let mut selector = JournalSelector::new(choices);
            let terminal = draw(&mut selector, width, 60);
            let buffer = terminal.backend().buffer();
            let label = buffer
                .content()
                .windows(6)
                .position(|cells| {
                    cells.iter().map(|cell| cell.symbol()).collect::<String>() == "Locked"
                })
                .unwrap();
            let x = label % usize::from(width);
            let y = label / usize::from(width);
            let cells = &buffer.content()[y * usize::from(width)..(y + 1) * usize::from(width)];
            assert_eq!(cells[x + 6].symbol(), " ");
            assert_eq!(cells[x + 7].symbol(), "│");
            let marker = cells.iter().position(|cell| cell.symbol() == ">").unwrap();
            assert_eq!(cells[marker].style(), cells[x].style());
            assert!(cells[..x].iter().any(|cell| cell.symbol() == "…"));
        }
        for (width, height) in [(1, 1), (10, 4), (19, 3)] {
            let mut choices = journals();
            choices[0].locked = true;
            draw(&mut JournalSelector::new(choices), width, height);
        }
    }

    #[test]
    fn small_screens_keep_selected_journal_and_highlighted_controls_visible() {
        for (width, height) in [(100, 30), (80, 24), (60, 18), (31, 12)] {
            let mut choices = journals();
            choices.extend((0..25).map(|index| JournalChoice {
                id: format!("journal-{index}"),
                name: format!("Journal {index}"),
                is_personal: false,
                locked: false,
            }));
            let mut selector = JournalSelector::new(choices);
            selector.handle_event(key(KeyCode::End));
            let terminal = draw(&mut selector, width, height);
            let output = screen(&terminal);
            for text in [
                "Select Journal",
                "Journal 24",
                "Open: Enter",
                "Add: n",
                "Delete: d",
                "Exit: q",
            ] {
                assert!(output.contains(text), "Missing {text} at {width}x{height}");
            }
            let buffer = terminal.backend().buffer();
            let start = output[..output.find("Open: Enter").unwrap()]
                .chars()
                .count()
                + "Open: ".len();
            assert_eq!(buffer.content()[start].fg, menu_key_style().fg.unwrap());
        }
        for (width, height) in [(1, 1), (10, 4), (19, 3)] {
            let mut selector = JournalSelector::new(journals());
            draw(&mut selector, width, height);
        }
    }
}

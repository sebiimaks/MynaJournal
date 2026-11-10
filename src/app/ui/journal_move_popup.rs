use std::path::PathBuf;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Margin, Rect},
    style::Style,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use tui_textarea::TextArea;

use super::{
    Styles,
    ui_functions::{centered_rect_exact_dimensions, menu_key_style},
};
use crate::{app::keymap::Input, journal_move::MoveRequest};

pub(crate) enum MoveJournalAction {
    Keep,
    Close,
    Prepare(PathBuf),
    Move(Box<MoveRequest>),
}

pub(crate) struct MoveJournalPopup {
    source: PathBuf,
    editor: TextArea<'static>,
    request: Option<MoveRequest>,
    error: Option<String>,
    result: Option<(String, bool)>,
    scroll: u16,
    scroll_max: u16,
    page_size: u16,
}

impl MoveJournalPopup {
    pub(crate) fn new(source: PathBuf) -> Self {
        Self {
            source,
            editor: TextArea::default(),
            request: None,
            error: None,
            result: None,
            scroll: 0,
            scroll_max: 0,
            page_size: 1,
        }
    }

    pub(crate) fn prepared(&mut self, request: MoveRequest) {
        self.source = request.source.clone();
        self.request = Some(request);
        self.error = None;
        self.reset_scroll();
    }

    pub(crate) fn show_error(&mut self, error: String) {
        self.request = None;
        self.error = Some(error);
        self.reset_scroll();
    }

    pub(crate) fn result(message: String, error: bool) -> Self {
        let mut popup = Self::new(PathBuf::new());
        popup.result = Some((message, error));
        popup
    }

    fn reset_scroll(&mut self) {
        self.scroll = 0;
        self.scroll_max = 0;
        self.page_size = 1;
    }

    pub(crate) fn handle_input(&mut self, input: &Input) -> MoveJournalAction {
        let ctrl = input.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && input.key_code == KeyCode::Char('c') {
            return MoveJournalAction::Close;
        }
        if input.key_code == KeyCode::Esc {
            if self.request.take().is_some() {
                self.reset_scroll();
                return MoveJournalAction::Keep;
            }
            return MoveJournalAction::Close;
        }
        if input
            .modifiers
            .intersects(KeyModifiers::ALT | KeyModifiers::SUPER)
        {
            return MoveJournalAction::Keep;
        }
        let enter =
            input.key_code == KeyCode::Enter || (ctrl && input.key_code == KeyCode::Char('m'));
        if self.result.is_some() && enter {
            return MoveJournalAction::Close;
        }
        if self.request.is_some() && !ctrl {
            match input.key_code {
                KeyCode::Char('y' | 'Y') => {
                    return MoveJournalAction::Move(Box::new(self.request.take().unwrap()));
                }
                KeyCode::Char('n' | 'N') => return MoveJournalAction::Close,
                _ => {}
            }
        }
        if !ctrl {
            let scroll = match input.key_code {
                KeyCode::Up => Some(self.scroll.saturating_sub(1)),
                KeyCode::Down => Some(self.scroll.saturating_add(1)),
                KeyCode::PageUp => Some(self.scroll.saturating_sub(self.page_size)),
                KeyCode::PageDown => Some(self.scroll.saturating_add(self.page_size)),
                KeyCode::Home if self.request.is_some() || self.result.is_some() => Some(0),
                KeyCode::End if self.request.is_some() || self.result.is_some() => {
                    Some(self.scroll_max)
                }
                _ => None,
            };
            if let Some(scroll) = scroll {
                self.scroll = scroll.min(self.scroll_max);
                return MoveJournalAction::Keep;
            }
        }
        if self.request.is_some() || self.result.is_some() {
            return MoveJournalAction::Keep;
        }
        if enter {
            let path = self.editor.lines()[0].clone();
            if path.trim().is_empty() {
                self.show_error("Enter the full path of a new destination folder.".into());
                return MoveJournalAction::Keep;
            }
            return MoveJournalAction::Prepare(PathBuf::from(path));
        }
        match input.key_code {
            KeyCode::Tab | KeyCode::BackTab => {}
            KeyCode::Char('i' | 'j') if ctrl => {}
            KeyCode::Char(ch) if ch.is_control() => {}
            _ => {
                if self.editor.input(KeyEvent::from(input)) {
                    self.error = None;
                    self.reset_scroll();
                }
            }
        }
        MoveJournalAction::Keep
    }

    fn footer(&self) -> Line<'static> {
        let commands: &[(&str, &str)] = if self.result.is_some() {
            &[("Enter/Esc", "close"), ("↑/↓", "scroll")]
        } else if self.request.is_some() {
            &[
                ("Y", "move"),
                ("N", "cancel"),
                ("Esc", "edit"),
                ("↑/↓", "scroll"),
            ]
        } else {
            &[("Enter", "review"), ("Esc", "cancel"), ("↑/↓", "scroll")]
        };
        let mut spans = Vec::new();
        for (index, (key, action)) in commands.iter().enumerate() {
            if index > 0 {
                spans.push(Span::raw(" | "));
            }
            spans.push(Span::styled(*key, menu_key_style()));
            spans.push(Span::raw(format!(": {action}")));
        }
        Line::from(spans)
    }

    fn details(&self, styles: &Styles) -> Text<'static> {
        if let Some((message, error)) = &self.result {
            let text = Text::from(safe_text(message));
            return if *error {
                text.style(Style::default().fg(styles.msgbox.error))
            } else {
                text
            };
        }
        let mut lines = Vec::new();
        if let Some(error) = &self.error {
            lines.extend(
                Text::from(safe_text(error))
                    .lines
                    .into_iter()
                    .map(|line| line.style(Style::default().fg(styles.msgbox.error))),
            );
            lines.push(Line::default());
        }
        let message = if let Some(request) = &self.request {
            format!(
                "From: {}\n\nNew folder: {}\nStorage: {}\n\nThe journal will close during the move. Its data and encryption header stay together. The original is retained as a recovery copy.\n\nMove this journal?",
                self.source.display(),
                request.destination.display(),
                request.storage_destination.display(),
            )
        } else {
            format!(
                "Current storage: {}\n\nEnter an absolute path for a new folder. Its parent must already exist. The destination must not exist.\n\nThe next screen shows the resulting storage path before you confirm.",
                self.source.display(),
            )
        };
        lines.extend(Text::from(safe_text(&message)).lines);
        Text::from(lines)
    }

    pub(crate) fn render_widget(&mut self, frame: &mut Frame, bounds: Rect, styles: &Styles) {
        let area = centered_rect_exact_dimensions(76, 18, bounds);
        frame.render_widget(Clear, area);
        let block = Block::default()
            .title(if self.result.is_some() {
                " Move Journal Result "
            } else {
                " Move Journal "
            })
            .borders(Borders::ALL)
            .style(Style::from(styles.general.input_block_active));
        frame.render_widget(block, area);
        let content = area.inner(Margin::new(2, 1));
        if content.is_empty() {
            return;
        }
        if content.width < 12 || content.height < 4 {
            frame.render_widget(
                Paragraph::new("Enlarge terminal. Esc closes.").wrap(Wrap { trim: false }),
                content,
            );
            return;
        }
        let editing = self.request.is_none() && self.result.is_none();
        let footer = self.footer();
        let editor_height = if editing { 3 } else { 0 };
        let footer_height = u16::try_from(footer.width().div_ceil(usize::from(content.width)))
            .unwrap_or(u16::MAX)
            .min(content.height.saturating_sub(editor_height + 1));
        let details_area = Rect::new(
            content.x,
            content.y + editor_height,
            content.width,
            content.height - editor_height - footer_height,
        );
        let footer_area = Rect::new(
            content.x,
            details_area.bottom(),
            content.width,
            footer_height,
        );
        frame.render_widget(
            Paragraph::new(footer).wrap(Wrap { trim: false }),
            footer_area,
        );
        if editing {
            self.editor.set_block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("New destination folder"),
            );
            self.editor.set_cursor_line_style(Style::default());
            self.editor
                .set_style(Style::from(styles.general.input_block_active));
            self.editor
                .set_cursor_style(Style::from(styles.general.input_cursor_active));
            frame.render_widget(
                &self.editor,
                Rect::new(content.x, content.y, content.width, editor_height),
            );
        }
        let paragraph = Paragraph::new(self.details(styles)).wrap(Wrap { trim: false });
        self.page_size = details_area.height.max(1);
        self.scroll_max = u16::try_from(
            paragraph
                .line_count(details_area.width)
                .saturating_sub(usize::from(details_area.height)),
        )
        .unwrap_or(u16::MAX);
        self.scroll = self.scroll.min(self.scroll_max);
        frame.render_widget(paragraph.scroll((self.scroll, 0)), details_area);
    }
}

fn safe_text(text: &str) -> String {
    text.chars()
        .filter(|ch| !ch.is_control() || *ch == '\n')
        .collect()
}

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use super::*;

    fn key(popup: &mut MoveJournalPopup, code: KeyCode) -> MoveJournalAction {
        popup.handle_input(&Input::new(code, KeyModifiers::NONE))
    }

    fn render(popup: &mut MoveJournalPopup, width: u16, height: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal
            .draw(|frame| popup.render_widget(frame, frame.area(), &Styles::default()))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[cfg(feature = "file")]
    fn request_fixture() -> (tempfile::TempDir, MoveRequest) {
        use crate::settings::{ConfigContext, Settings, document::ConfigDocument};
        let directory = tempfile::tempdir().unwrap();
        let profile = directory.path().join("profile");
        let source = directory.path().join("日記 entries");
        std::fs::create_dir(&profile).unwrap();
        std::fs::create_dir(&source).unwrap();
        let settings: Settings = toml::from_str(&format!(
            "backend_type = 'File'\n[file_backend]\ndirectory = '{}'\n",
            source.display()
        ))
        .unwrap();
        let settings_path = profile.join("config.toml");
        std::fs::write(&settings_path, toml::to_string(&settings).unwrap()).unwrap();
        let context = ConfigContext {
            journal_name: Some("Personal".into()),
            return_to_journals: true,
            storage_overridden: false,
            settings_path: settings_path.clone(),
            themes_path: profile.join("themes"),
            active_theme_id: Some("tui".into()),
            theme_warning: None,
            launch_details: vec![],
        };
        let document = ConfigDocument::load(settings_path).unwrap();
        let request = crate::journal_move::prepare(
            &settings,
            &context,
            &document,
            &directory.path().join("new destination"),
        )
        .unwrap();
        (directory, request)
    }

    #[cfg(feature = "file")]
    #[test]
    fn reviewed_move_requires_y_and_escape_returns_to_the_unchanged_path_editor() {
        let (_directory, request) = request_fixture();
        let mut popup = MoveJournalPopup::new(request.source.clone());
        let destination = request.destination.to_string_lossy().to_string();
        for ch in destination.chars() {
            key(&mut popup, KeyCode::Char(ch));
        }
        popup.prepared(request.clone());
        let text = render(&mut popup, 100, 30);
        assert!(
            text.contains("From:") && text.contains("Storage:") && text.contains("recovery copy"),
            "{text}"
        );
        assert!(matches!(
            key(&mut popup, KeyCode::Enter),
            MoveJournalAction::Keep
        ));
        assert!(matches!(
            key(&mut popup, KeyCode::Esc),
            MoveJournalAction::Keep
        ));
        assert_eq!(popup.editor.lines(), [destination.as_str()]);
        assert!(popup.request.is_none());
        popup.prepared(request.clone());
        assert!(matches!(
            key(&mut popup, KeyCode::Char('n')),
            MoveJournalAction::Close
        ));
        assert!(!request.destination.exists());
        let MoveJournalAction::Move(confirmed) = key(&mut popup, KeyCode::Char('Y')) else {
            panic!("Move was not confirmed")
        };
        assert_eq!(confirmed.source, request.source);
        assert_eq!(confirmed.destination, request.destination);
        assert_eq!(confirmed.storage_destination, request.storage_destination);
        assert!(!request.destination.exists());
    }

    #[cfg(feature = "file")]
    #[test]
    fn confirmation_paths_are_scrollable_and_commands_use_the_main_menu_style() {
        let (_directory, request) = request_fixture();
        let mut popup = MoveJournalPopup::new(request.source.clone());
        popup.prepared(request);
        render(&mut popup, 30, 8);
        assert!(popup.scroll_max > 0);
        key(&mut popup, KeyCode::End);
        let text = render(&mut popup, 30, 8);
        assert!(text.contains("Move this journal?"), "{text}");
        assert!(
            text.contains("Y: move") && text.contains("Esc:") && text.contains("edit"),
            "{text}"
        );
        assert!(
            popup
                .footer()
                .spans
                .iter()
                .filter(|span| span.style == menu_key_style())
                .map(|span| span.content.as_ref())
                .eq(["Y", "N", "Esc", "↑/↓"])
        );
    }

    #[test]
    fn path_entry_requires_review_and_keeps_unicode_and_popup_shortcuts_literal() {
        let mut popup = MoveJournalPopup::new("/old/entries".into());
        assert!(matches!(
            key(&mut popup, KeyCode::Enter),
            MoveJournalAction::Keep
        ));
        assert!(popup.error.is_some());
        for ch in "/new/日記 q!y".chars() {
            assert!(matches!(
                key(&mut popup, KeyCode::Char(ch)),
                MoveJournalAction::Keep
            ));
        }
        assert!(popup.error.is_none());
        let MoveJournalAction::Prepare(path) = key(&mut popup, KeyCode::Enter) else {
            panic!("Expected review request")
        };
        assert_eq!(path, PathBuf::from("/new/日記 q!y"));
        assert!(matches!(
            key(&mut popup, KeyCode::Esc),
            MoveJournalAction::Close
        ));
    }

    #[test]
    fn path_editor_stays_on_one_line_and_control_c_cancels() {
        let mut popup = MoveJournalPopup::new("/old/entries".into());
        for code in [KeyCode::Char('i'), KeyCode::Char('j'), KeyCode::Char('\n')] {
            popup.handle_input(&Input::new(code, KeyModifiers::CONTROL));
        }
        key(&mut popup, KeyCode::Tab);
        assert_eq!(popup.editor.lines(), [""]);
        assert!(matches!(
            popup.handle_input(&Input::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            MoveJournalAction::Close
        ));
    }

    #[test]
    fn long_errors_and_result_paths_scroll_without_hiding_close_controls() {
        let mut popup = MoveJournalPopup::result(
            format!(
                "{}\nFinal path: /日記/destination\u{1b}\u{7}",
                "Long error detail. ".repeat(70)
            ),
            true,
        );
        let before = render(&mut popup, 30, 8);
        assert!(before.contains("Enter/Esc") && before.contains("close"));
        assert!(popup.scroll_max > 0);
        key(&mut popup, KeyCode::PageDown);
        assert_eq!(popup.scroll, popup.page_size);
        key(&mut popup, KeyCode::End);
        let after = render(&mut popup, 30, 8);
        assert!(after.contains("destination"), "{after}");
        assert!(!after.contains(['\u{1b}', '\u{7}']));
        assert!(matches!(
            key(&mut popup, KeyCode::Char('y')),
            MoveJournalAction::Keep
        ));
        assert!(matches!(
            key(&mut popup, KeyCode::Enter),
            MoveJournalAction::Close
        ));
        key(&mut popup, KeyCode::Home);
        assert_eq!(popup.scroll, 0);
    }

    #[test]
    fn error_keeps_destination_editable_and_tiny_windows_render_safely() {
        let mut popup = MoveJournalPopup::new("/old/entries".into());
        for ch in "/new/path".chars() {
            key(&mut popup, KeyCode::Char(ch));
        }
        popup.show_error("Destination already exists.\u{1b}".into());
        assert_eq!(popup.editor.lines(), ["/new/path"]);
        let text = render(&mut popup, 80, 24);
        assert!(text.contains("Destination already exists."));
        assert!(!text.contains('\u{1b}'));
        for (width, height) in [(1, 1), (10, 3), (30, 8)] {
            render(&mut popup, width, height);
        }
    }
}

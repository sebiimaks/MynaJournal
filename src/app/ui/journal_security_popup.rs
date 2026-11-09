use backend::protection::SecretString;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Alignment, Margin, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use zeroize::Zeroizing;

use super::{
    Styles,
    ui_functions::{centered_rect_exact_dimensions, menu_key_style},
};
use crate::app::keymap::Input;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum JournalPasswordAction {
    Unlock,
    UnlockEntry(u32),
    Enable,
    Change,
    Disable,
}

pub enum JournalSecurityAction {
    Keep,
    Close,
    Unlock(SecretString),
    UnlockEntry {
        id: u32,
        password: SecretString,
    },
    Enable(SecretString),
    Change {
        current: SecretString,
        new: SecretString,
    },
    Disable(SecretString),
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    ConfirmEnable,
    ConfirmDisable,
    Current,
    New,
    Confirm,
}

pub struct JournalSecurityPopup {
    action: JournalPasswordAction,
    stage: Stage,
    current: Zeroizing<String>,
    password: Zeroizing<String>,
    confirmation: Zeroizing<String>,
    error: Option<String>,
    pending_enable: Option<PendingEnable>,
    entry_title: String,
}

struct PendingEnable {
    password: Option<SecretString>,
    entries: Vec<u32>,
    unlocked: Vec<u32>,
}

impl JournalSecurityPopup {
    pub fn new(action: JournalPasswordAction) -> Self {
        Self {
            action,
            stage: Self::initial_stage(action),
            current: Zeroizing::new(String::new()),
            password: Zeroizing::new(String::new()),
            confirmation: Zeroizing::new(String::new()),
            error: None,
            pending_enable: None,
            entry_title: String::new(),
        }
    }

    fn initial_stage(action: JournalPasswordAction) -> Stage {
        match action {
            JournalPasswordAction::Enable => Stage::ConfirmEnable,
            JournalPasswordAction::Disable => Stage::ConfirmDisable,
            _ => Stage::Current,
        }
    }

    pub(super) fn consolidating_entries(&self) -> bool {
        self.pending_enable.is_some()
    }

    pub(super) fn entry_to_unlock(&self) -> Option<u32> {
        match self.action {
            JournalPasswordAction::UnlockEntry(id) => Some(id),
            _ => None,
        }
    }

    pub(super) fn set_entry_title(&mut self, title: &str) {
        self.entry_title = title
            .chars()
            .filter(|ch| !ch.is_control())
            .take(80)
            .collect();
    }

    pub(super) fn begin_consolidation(&mut self, password: SecretString, entries: Vec<u32>) {
        assert!(!entries.is_empty());
        self.action = JournalPasswordAction::UnlockEntry(entries[0]);
        self.stage = Stage::Current;
        self.error = None;
        self.pending_enable = Some(PendingEnable {
            password: Some(password),
            entries,
            unlocked: Vec::new(),
        });
    }

    pub(super) fn entry_unlocked(&mut self, id: u32) -> Option<SecretString> {
        let pending = self.pending_enable.as_mut()?;
        pending.unlocked.push(id);
        if let Some(&next) = pending.entries.get(pending.unlocked.len()) {
            self.action = JournalPasswordAction::UnlockEntry(next);
            self.stage = Stage::Current;
            self.error = None;
            None
        } else {
            pending.password.take()
        }
    }

    pub(super) fn cancel_consolidation(&mut self) -> Vec<u32> {
        self.action = JournalPasswordAction::Enable;
        self.stage = Stage::ConfirmEnable;
        self.pending_enable
            .take()
            .map_or_else(Vec::new, |pending| pending.unlocked)
    }

    pub fn show_error(&mut self, error: String) {
        self.current = Zeroizing::new(String::new());
        self.password = Zeroizing::new(String::new());
        self.confirmation = Zeroizing::new(String::new());
        self.stage = Self::initial_stage(self.action);
        self.error = Some(error);
    }

    pub fn handle_input(&mut self, input: &Input) -> JournalSecurityAction {
        let ctrl = input.modifiers.contains(KeyModifiers::CONTROL);
        if input.key_code == KeyCode::Esc || (ctrl && input.key_code == KeyCode::Char('c')) {
            return JournalSecurityAction::Close;
        }
        let confirm =
            input.key_code == KeyCode::Enter || (ctrl && input.key_code == KeyCode::Char('m'));
        if input
            .modifiers
            .intersects(KeyModifiers::ALT | KeyModifiers::SUPER)
            || (ctrl && !confirm)
        {
            return JournalSecurityAction::Keep;
        }
        if matches!(self.stage, Stage::ConfirmDisable | Stage::ConfirmEnable) {
            match input.key_code {
                KeyCode::Char('y' | 'Y') => {
                    self.stage = if self.stage == Stage::ConfirmEnable {
                        Stage::New
                    } else {
                        Stage::Current
                    };
                    self.error = None;
                }
                KeyCode::Char('n' | 'N') => return JournalSecurityAction::Close,
                _ => {}
            }
            return JournalSecurityAction::Keep;
        }
        if confirm {
            return self.submit();
        }
        match input.key_code {
            KeyCode::Backspace => {
                self.buffer_mut().pop();
                self.error = None;
            }
            KeyCode::Char(ch) if !ch.is_control() => {
                if self.buffer().len() + ch.len_utf8() <= 4096 {
                    self.buffer_mut().push(ch);
                    self.error = None;
                } else {
                    self.error = Some("Passwords can contain at most 4096 bytes.".into());
                }
            }
            _ => {}
        }
        JournalSecurityAction::Keep
    }

    fn buffer(&self) -> &str {
        match self.stage {
            Stage::Current => &self.current,
            Stage::Confirm => &self.confirmation,
            _ => &self.password,
        }
    }

    fn buffer_mut(&mut self) -> &mut String {
        match self.stage {
            Stage::Current => &mut self.current,
            Stage::Confirm => &mut self.confirmation,
            _ => &mut self.password,
        }
    }

    fn submit(&mut self) -> JournalSecurityAction {
        if self.buffer().is_empty() {
            self.error = Some("Enter a non-empty password.".into());
            return JournalSecurityAction::Keep;
        }
        match self.stage {
            Stage::Current => match self.action {
                JournalPasswordAction::Unlock => {
                    return JournalSecurityAction::Unlock(
                        std::mem::take(&mut *self.current).into(),
                    );
                }
                JournalPasswordAction::UnlockEntry(id) => {
                    return JournalSecurityAction::UnlockEntry {
                        id,
                        password: std::mem::take(&mut *self.current).into(),
                    };
                }
                JournalPasswordAction::Disable => {
                    return JournalSecurityAction::Disable(
                        std::mem::take(&mut *self.current).into(),
                    );
                }
                _ => self.stage = Stage::New,
            },
            Stage::New => self.stage = Stage::Confirm,
            Stage::Confirm if *self.password != *self.confirmation => {
                self.error = Some("Passwords do not match. Confirm again.".into());
                self.confirmation = Zeroizing::new(String::new());
                return JournalSecurityAction::Keep;
            }
            Stage::Confirm => {
                self.confirmation = Zeroizing::new(String::new());
                let password = std::mem::take(&mut *self.password).into();
                return if self.action == JournalPasswordAction::Enable {
                    JournalSecurityAction::Enable(password)
                } else {
                    JournalSecurityAction::Change {
                        current: std::mem::take(&mut *self.current).into(),
                        new: password,
                    }
                };
            }
            Stage::ConfirmDisable | Stage::ConfirmEnable => {}
        }
        self.error = None;
        JournalSecurityAction::Keep
    }

    pub fn render_widget(&self, frame: &mut Frame, bounds: Rect, styles: &Styles) {
        let area = centered_rect_exact_dimensions(56, 12, bounds);
        let style = Style::from(styles.general.input_block_active);
        let title = match self.action {
            JournalPasswordAction::Unlock => " Unlock Journal ",
            JournalPasswordAction::UnlockEntry(_) => " Unlock Entry ",
            JournalPasswordAction::Enable => " Protect Journal ",
            JournalPasswordAction::Change => " Change Journal Password ",
            JournalPasswordAction::Disable => " Remove Journal Protection ",
        };
        frame.render_widget(Clear, area);
        frame.render_widget(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .style(style),
            area,
        );
        let content = area.inner(Margin {
            horizontal: 2,
            vertical: 1,
        });
        if content.is_empty() {
            return;
        }
        let confirmation = matches!(self.stage, Stage::ConfirmDisable | Stage::ConfirmEnable);
        let footer = if confirmation {
            Line::from(vec![
                Span::styled("Y", menu_key_style()),
                Span::raw(": continue | "),
                Span::styled("Esc/N", menu_key_style()),
                Span::raw(": cancel"),
            ])
        } else {
            Line::from(vec![
                Span::styled("Enter", menu_key_style()),
                Span::raw(": continue | "),
                Span::styled("Esc", menu_key_style()),
                Span::raw(": cancel"),
            ])
        };
        let footer_height = (footer.width().div_ceil(usize::from(content.width)) as u16)
            .min(content.height.saturating_sub(3));
        let body = Rect::new(
            content.x,
            content.y,
            content.width,
            content.height - footer_height,
        );
        let footer_area = Rect::new(content.x, body.bottom(), content.width, footer_height);
        frame.render_widget(
            Paragraph::new(footer).wrap(Wrap { trim: false }),
            footer_area,
        );
        if confirmation {
            let mut lines = if self.stage == Stage::ConfirmEnable {
                vec![
                    Line::from("Protect this journal with one password?"),
                    Line::from("Existing entry passwords will be replaced."),
                ]
            } else {
                vec![
                    Line::from("Store this journal as plaintext?"),
                    Line::from("Your current password is required."),
                ]
            };
            if body.height >= 6 {
                lines.insert(1, Line::default());
            }
            if let Some(error) = &self.error {
                lines.push(Line::styled(
                    error,
                    Style::default().fg(styles.msgbox.error),
                ));
            }
            frame.render_widget(
                Paragraph::new(lines)
                    .alignment(Alignment::Center)
                    .wrap(Wrap { trim: false }),
                body,
            );
            return;
        }
        let width = 32.min(body.width);
        let field_area = Rect::new(
            body.x + (body.width - width) / 2,
            body.y + u16::from(body.height >= 7),
            width,
            body.height.min(3),
        );
        let label = match self.stage {
            Stage::New => " New Password ",
            Stage::Confirm => " Confirm Password ",
            _ if self.action == JournalPasswordAction::Unlock => " Password ",
            _ => " Current Password ",
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(label)
            .style(style);
        let input = block.inner(field_area);
        let length = self
            .buffer()
            .chars()
            .count()
            .min(usize::from(input.width.saturating_sub(1)));
        frame.render_widget(Paragraph::new("*".repeat(length)).block(block), field_area);
        if !input.is_empty() {
            frame.set_cursor_position((input.x + length as u16, input.y));
        }
        let message = if let Some(error) = &self.error {
            Line::styled(error, Style::default().fg(styles.msgbox.error))
        } else if matches!(self.stage, Stage::New | Stage::Confirm) {
            Line::from("Password cannot be recovered.")
        } else if let JournalPasswordAction::UnlockEntry(id) = self.action {
            Line::from(if self.entry_title.is_empty() {
                format!("Unlock entry {id} to continue.")
            } else {
                self.entry_title.clone()
            })
        } else {
            Line::default()
        };
        let info_y = field_area
            .bottom()
            .saturating_add(u16::from(body.height >= 6));
        let info = Rect::new(
            body.x,
            info_y,
            body.width,
            body.bottom().saturating_sub(info_y),
        );
        frame.render_widget(
            Paragraph::new(message)
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: false }),
            info,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use backend::protection::ExposeSecret;
    use ratatui::{Terminal, backend::TestBackend};

    fn key(popup: &mut JournalSecurityPopup, code: KeyCode) -> JournalSecurityAction {
        popup.handle_input(&Input::new(code, KeyModifiers::NONE))
    }

    fn type_password(popup: &mut JournalSecurityPopup, text: &str) {
        for ch in text.chars() {
            key(popup, KeyCode::Char(ch));
        }
    }

    #[test]
    fn enabling_requires_nonempty_matching_confirmation() {
        let mut popup = JournalSecurityPopup::new(JournalPasswordAction::Enable);
        key(&mut popup, KeyCode::Char('y'));
        assert!(matches!(
            key(&mut popup, KeyCode::Enter),
            JournalSecurityAction::Keep
        ));
        type_password(&mut popup, "secret");
        key(&mut popup, KeyCode::Enter);
        type_password(&mut popup, "different");
        assert!(matches!(
            key(&mut popup, KeyCode::Enter),
            JournalSecurityAction::Keep
        ));
        assert!(popup.confirmation.is_empty());
        type_password(&mut popup, "secret");
        let JournalSecurityAction::Enable(password) = key(&mut popup, KeyCode::Enter) else {
            panic!("Expected enable")
        };
        assert_eq!(password.expose_secret(), "secret");
        assert!(popup.password.is_empty() && popup.confirmation.is_empty());
    }

    #[test]
    fn changing_requires_current_and_confirmed_new_password_and_errors_clear_all_buffers() {
        let mut popup = JournalSecurityPopup::new(JournalPasswordAction::Change);
        for text in ["current", "new"] {
            type_password(&mut popup, text);
            assert!(matches!(
                key(&mut popup, KeyCode::Enter),
                JournalSecurityAction::Keep
            ));
        }
        type_password(&mut popup, "new");
        let JournalSecurityAction::Change { current, new } = key(&mut popup, KeyCode::Enter) else {
            panic!("Expected change")
        };
        assert_eq!(current.expose_secret(), "current");
        assert_eq!(new.expose_secret(), "new");
        type_password(&mut popup, "partial retry");
        popup.show_error("Password is incorrect".into());
        assert!(
            popup.current.is_empty() && popup.password.is_empty() && popup.confirmation.is_empty()
        );
        assert!(popup.stage == Stage::Current);
        assert!(matches!(
            key(&mut popup, KeyCode::Esc),
            JournalSecurityAction::Close
        ));
    }

    #[test]
    fn password_input_rejects_control_shortcuts_and_limits_utf8_bytes() {
        let mut popup = JournalSecurityPopup::new(JournalPasswordAction::Unlock);
        for _ in 0..1365 {
            type_password(&mut popup, "界");
        }
        type_password(&mut popup, "界");
        assert_eq!(popup.current.len(), 4095);
        key(&mut popup, KeyCode::Backspace);
        assert_eq!(popup.current.len(), 4092);
        popup.handle_input(&Input::new(KeyCode::Char('x'), KeyModifiers::CONTROL));
        popup.handle_input(&Input::new(KeyCode::Char('x'), KeyModifiers::ALT));
        assert_eq!(popup.current.len(), 4092);
        assert!(matches!(
            popup.handle_input(&Input::new(KeyCode::Char('m'), KeyModifiers::CONTROL)),
            JournalSecurityAction::Unlock(_)
        ));
    }

    #[test]
    fn consolidation_keeps_journal_secret_while_entry_errors_retry_and_cancel_reports_opened_entries()
     {
        let mut popup = JournalSecurityPopup::new(JournalPasswordAction::Enable);
        popup.begin_consolidation("journal secret".to_owned().into(), vec![4, 8]);
        assert!(popup.consolidating_entries());
        assert!(popup.entry_unlocked(4).is_none());
        popup.show_error("Incorrect entry password".into());
        assert!(popup.action == JournalPasswordAction::UnlockEntry(8));
        assert!(popup.consolidating_entries());
        assert_eq!(popup.cancel_consolidation(), vec![4]);
        assert!(!popup.consolidating_entries());
        popup.begin_consolidation("another journal secret".to_owned().into(), vec![4, 8]);
        assert!(popup.entry_unlocked(4).is_none());
        let password = popup.entry_unlocked(8).unwrap();
        assert_eq!(password.expose_secret(), "another journal secret");
        assert!(popup.pending_enable.as_ref().unwrap().password.is_none());
        assert_eq!(popup.cancel_consolidation(), vec![4, 8]);
    }

    #[test]
    fn removing_requires_explicit_plaintext_confirmation_and_password() {
        let mut popup = JournalSecurityPopup::new(JournalPasswordAction::Disable);
        key(&mut popup, KeyCode::Enter);
        assert!(popup.stage == Stage::ConfirmDisable);
        key(&mut popup, KeyCode::Char('y'));
        assert!(matches!(
            key(&mut popup, KeyCode::Enter),
            JournalSecurityAction::Keep
        ));
        type_password(&mut popup, "current");
        assert!(matches!(
            key(&mut popup, KeyCode::Enter),
            JournalSecurityAction::Disable(_)
        ));
    }

    #[test]
    fn passwords_are_masked_at_all_terminal_sizes() {
        for (width, height) in [(160, 60), (56, 12), (30, 8), (12, 4), (1, 1)] {
            let mut popup = JournalSecurityPopup::new(JournalPasswordAction::Unlock);
            type_password(&mut popup, "private-password-界");
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal
                .draw(|frame| popup.render_widget(frame, frame.area(), &Styles::default()))
                .unwrap();
            let screen = terminal
                .backend()
                .buffer()
                .content()
                .iter()
                .map(|cell| cell.symbol())
                .collect::<String>();
            assert!(!screen.contains("private") && !screen.contains('界'));
            if width >= 30 {
                assert!(screen.contains("***"));
            }
        }
    }

    #[test]
    fn short_terminal_keeps_full_protection_confirmation_and_controls_visible() {
        for (action, warning) in [
            (
                JournalPasswordAction::Enable,
                "Existing entry passwords will be replaced.",
            ),
            (
                JournalPasswordAction::Disable,
                "Your current password is required.",
            ),
        ] {
            let popup = JournalSecurityPopup::new(action);
            let mut terminal = Terminal::new(TestBackend::new(30, 8)).unwrap();
            terminal
                .draw(|frame| popup.render_widget(frame, frame.area(), &Styles::default()))
                .unwrap();
            let buffer = terminal.backend().buffer();
            let text = (1..7)
                .map(|y| (2..28).map(|x| buffer[(x, y)].symbol()).collect::<String>())
                .collect::<Vec<_>>()
                .join(" ");
            let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
            assert!(text.contains(warning), "Missing warning in {text}");
            assert!(
                text.contains("Y: continue") && text.contains("Esc/N: cancel"),
                "Missing controls in {text}"
            );
        }
    }
}

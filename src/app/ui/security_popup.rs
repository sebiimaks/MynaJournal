use backend::protection::SecretString;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Alignment, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use zeroize::Zeroizing;

use super::{Styles, ui_functions::centered_rect_exact_dimensions};
use crate::app::keymap::Input;

#[derive(Clone, Copy)]
pub(super) enum PasswordAction {
    Protect,
    Unlock,
    Change,
}

pub(super) enum SecurityAction {
    Keep,
    Close,
    Lock,
    Remove,
    DiscardDraft,
    Protect(SecretString),
    Unlock(SecretString),
    Change(SecretString),
}

enum Screen {
    Menu,
    Password {
        action: PasswordAction,
        confirm: bool,
    },
    ConfirmRemove,
    ConfirmDiscard,
}

pub struct SecurityPopup {
    pub(super) entry_id: u32,
    protected: bool,
    locked: bool,
    draft: bool,
    screen: Screen,
    password: Zeroizing<String>,
    confirmation: Zeroizing<String>,
    error: Option<String>,
}

impl SecurityPopup {
    pub(super) fn new(entry_id: u32, protected: bool, locked: bool, draft: bool) -> Self {
        Self {
            entry_id,
            protected,
            locked,
            draft,
            screen: Screen::Menu,
            password: Zeroizing::new(String::new()),
            confirmation: Zeroizing::new(String::new()),
            error: None,
        }
    }

    pub(super) fn prompt(&mut self, action: PasswordAction) {
        self.screen = Screen::Password {
            action,
            confirm: false,
        };
        self.password = Zeroizing::new(String::new());
        self.confirmation = Zeroizing::new(String::new());
        self.error = None;
    }

    pub(super) fn show_error(&mut self, error: String) {
        self.error = Some(error);
        self.password = Zeroizing::new(String::new());
        self.confirmation = Zeroizing::new(String::new());
        if let Screen::Password { confirm, .. } = &mut self.screen {
            *confirm = false;
        }
    }

    pub(super) fn handle_input(&mut self, input: &Input) -> SecurityAction {
        if input.key_code == KeyCode::Esc {
            return SecurityAction::Close;
        }
        match self.screen {
            Screen::Menu => match input.key_code {
                KeyCode::Enter if !self.protected => self.prompt(PasswordAction::Protect),
                KeyCode::Char('p' | 'P') if !self.protected => self.prompt(PasswordAction::Protect),
                KeyCode::Char('u' | 'U') | KeyCode::Enter if self.locked => {
                    self.prompt(PasswordAction::Unlock)
                }
                KeyCode::Char('l' | 'L') | KeyCode::Enter if self.protected && !self.locked => {
                    return SecurityAction::Lock;
                }
                KeyCode::Char('c' | 'C') if self.protected && !self.locked => {
                    self.prompt(PasswordAction::Change)
                }
                KeyCode::Char('r' | 'R') if self.protected && !self.locked => {
                    self.screen = Screen::ConfirmRemove
                }
                KeyCode::Char('d' | 'D') if self.draft => self.screen = Screen::ConfirmDiscard,
                _ => {}
            },
            Screen::ConfirmRemove if matches!(input.key_code, KeyCode::Char('y' | 'Y')) => {
                return SecurityAction::Remove;
            }
            Screen::ConfirmDiscard if matches!(input.key_code, KeyCode::Char('y' | 'Y')) => {
                return SecurityAction::DiscardDraft;
            }
            Screen::ConfirmRemove | Screen::ConfirmDiscard => {
                if matches!(input.key_code, KeyCode::Char('n' | 'N')) {
                    self.screen = Screen::Menu;
                }
            }
            Screen::Password { action, confirm } => {
                let buffer = if confirm {
                    &mut self.confirmation
                } else {
                    &mut self.password
                };
                match input.key_code {
                    KeyCode::Backspace => {
                        buffer.pop();
                    }
                    KeyCode::Char(ch)
                        if !input
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                            && !ch.is_control() =>
                    {
                        if buffer.len() + ch.len_utf8() <= 4096 {
                            buffer.push(ch);
                        } else {
                            self.error = Some("Passwords can contain at most 4096 bytes.".into());
                        }
                    }
                    KeyCode::Enter => {
                        if self.password.is_empty() {
                            self.error = Some("Enter a non-empty password.".into());
                        } else if matches!(action, PasswordAction::Unlock) {
                            return SecurityAction::Unlock(
                                std::mem::take(&mut *self.password).into(),
                            );
                        } else if !confirm {
                            self.screen = Screen::Password {
                                action,
                                confirm: true,
                            };
                            self.error = None;
                        } else if *self.password != *self.confirmation {
                            self.error = Some("Passwords do not match. Confirm again.".into());
                            self.confirmation = Zeroizing::new(String::new());
                        } else {
                            let password = std::mem::take(&mut *self.password).into();
                            return match action {
                                PasswordAction::Protect => SecurityAction::Protect(password),
                                _ => SecurityAction::Change(password),
                            };
                        }
                    }
                    _ => {}
                }
            }
        }
        SecurityAction::Keep
    }

    pub fn render_widget(&self, frame: &mut Frame, area: Rect, styles: &Styles) {
        let area = centered_rect_exact_dimensions(56, 14, area);
        frame.render_widget(Clear, area);
        let style = Style::from(styles.general.input_block_active);
        let title = match self.screen {
            Screen::Password {
                action: PasswordAction::Protect,
                ..
            } => " Protect Note ",
            Screen::Password {
                action: PasswordAction::Unlock,
                ..
            } => " Unlock Note ",
            Screen::Password {
                action: PasswordAction::Change,
                ..
            } => " Change Password ",
            _ => " Note Security ",
        };
        frame.render_widget(
            Block::default()
                .borders(Borders::ALL)
                .title(title)
                .style(style),
            area,
        );
        if let Screen::Password { action, confirm } = self.screen {
            self.render_password(frame, area, style, styles, action, confirm);
            return;
        }
        let mut lines = vec![Line::from("Title and date stay visible."), Line::default()];
        let action_line = |key: &'static str, caption: &'static str| {
            Line::from(vec![
                Span::styled(format!("[{key}] "), style.add_modifier(Modifier::BOLD)),
                Span::raw(caption),
            ])
        };
        match self.screen {
            Screen::Menu => {
                if !self.protected { lines.push(action_line("P", "Protect with password")); }
                else if self.locked { lines.push(action_line("U", "Unlock this note")); }
                else {
                    lines.push(action_line("L", "Lock now"));
                    lines.push(action_line("C", "Change password"));
                    lines.push(action_line("R", "Remove protection"));
                }
                if self.draft { lines.push(action_line("D", "Discard unsaved encrypted draft")); }
            }
            Screen::Password { .. } => unreachable!("password field rendered above"),
            Screen::ConfirmRemove => lines.push(Line::from("Store this note as plaintext? Press Y to remove protection, N to go back.")),
            Screen::ConfirmDiscard => lines.push(Line::from("Discard the unsaved encrypted draft? The saved note is kept. Y to discard, N to go back.")),
        }
        if let Some(error) = &self.error {
            lines.push(Line::styled(
                error,
                Style::default().fg(styles.msgbox.error),
            ));
        }
        lines.push(Line::default());
        lines.push(Line::from(match self.screen {
            Screen::Menu if !self.protected => "Enter: protect | Esc: cancel",
            Screen::Menu if self.locked => "Enter: unlock | Esc: cancel",
            Screen::Menu => "Enter: lock | Esc: cancel",
            _ => "Esc: cancel",
        }));
        frame.render_widget(
            Paragraph::new(lines).wrap(Wrap { trim: false }),
            area.inner(Margin {
                horizontal: 2,
                vertical: 1,
            }),
        );
    }

    fn render_password(
        &self,
        frame: &mut Frame,
        area: Rect,
        style: Style,
        styles: &Styles,
        action: PasswordAction,
        confirm: bool,
    ) {
        let content = area.inner(Margin {
            horizontal: 2,
            vertical: 1,
        });
        if content.is_empty() {
            return;
        }
        let footer = match (action, confirm) {
            (PasswordAction::Unlock, _) => "Enter: unlock note | Esc: cancel",
            (PasswordAction::Protect, true) => "Enter: protect note | Esc: cancel",
            (PasswordAction::Change, true) => "Enter: change password | Esc: cancel",
            (_, false) => "Enter: confirm password | Esc: cancel",
        };
        let footer = if footer.len() > usize::from(content.width) {
            footer.replace(" | ", "\n")
        } else {
            footer.to_owned()
        };
        let footer_height = (textwrap::wrap(&footer, usize::from(content.width)).len() as u16)
            .min(content.height.saturating_sub(3));
        let info = if let Some(error) = &self.error {
            vec![Line::styled(
                error,
                Style::default().fg(styles.msgbox.error),
            )]
        } else {
            let mut lines = vec![Line::from("Title and date stay visible.")];
            if confirm {
                lines.push(Line::from("Re-enter the same password."));
            }
            if !matches!(action, PasswordAction::Unlock) {
                lines.push(Line::from("Password cannot be recovered."));
            }
            lines
        };
        let body_height = content.height - footer_height;
        let input_height = body_height.min(3);
        let info_height = info
            .iter()
            .map(|line| textwrap::wrap(&line.to_string(), usize::from(content.width)).len())
            .sum::<usize>()
            .min(usize::from(body_height - input_height)) as u16;
        let space = body_height - input_height - info_height;
        let field_width = (area.width / 2).max(28).min(content.width);
        let field_area = Rect::new(
            content.x + (content.width - field_width) / 2,
            content.y + space / 3,
            field_width,
            input_height,
        );
        let info_area = Rect::new(
            content.x,
            field_area.bottom() + space.div_ceil(3),
            content.width,
            info_height,
        );
        let footer_area = Rect::new(
            content.x,
            content.bottom() - footer_height,
            content.width,
            footer_height,
        );
        let label = if confirm {
            " Confirm Password "
        } else if matches!(action, PasswordAction::Unlock) {
            " Password "
        } else {
            " New Password "
        };
        let block = Block::default()
            .borders(Borders::ALL)
            .title(label)
            .style(style);
        let input_area = block.inner(field_area);
        let buffer = if confirm {
            &self.confirmation
        } else {
            &self.password
        };
        let masked_length = buffer
            .chars()
            .count()
            .min(usize::from(input_area.width.saturating_sub(1)));
        frame.render_widget(
            Paragraph::new("*".repeat(masked_length)).block(block),
            field_area,
        );
        if !input_area.is_empty() {
            frame.set_cursor_position((input_area.x + masked_length as u16, input_area.y));
        }
        frame.render_widget(
            Paragraph::new(info)
                .alignment(Alignment::Center)
                .wrap(Wrap { trim: false }),
            info_area,
        );
        frame.render_widget(
            Paragraph::new(footer).wrap(Wrap { trim: false }),
            footer_area,
        );
    }
}

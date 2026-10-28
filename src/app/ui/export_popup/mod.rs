use std::{env, path::PathBuf};

use backend::{DataProvider, Entry};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::Style,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use tui_textarea::{CursorMove, TextArea};

use crate::app::{App, export::ExportTarget, keymap::Input};

use super::{PopupReturn, Styles, ui_functions::centered_rect_exact_height};

type ExportPopupInputReturn = PopupReturn<(ExportTarget, Option<u32>)>;

const FOOTER_TEXT: &str = "Enter: confirm | Esc or <Ctrl-c>: Cancel";
const FOOTER_MARGINE: u16 = 8;
const DEFAULT_FILE_NAME: &str = "mjournal_export.json";

pub struct ExportPopup<'a> {
    path_txt: TextArea<'a>,
    path_err_msg: String,
    entry_id: Option<u32>,
    paragraph_text: String,
    replacement: Option<ExportTarget>,
}

impl ExportPopup<'_> {
    pub fn create_entry_content<D: DataProvider>(
        entry: &Entry,
        app: &App<D>,
    ) -> anyhow::Result<Self> {
        let mut default_path = if let Some(path) = &app.settings.export.default_path {
            path.clone()
        } else {
            env::current_dir()?
        };

        if default_path.is_dir() || has_trailing_separator(&default_path) {
            default_path.push(suggested_filename(entry));
        }

        let mut path_txt = TextArea::new(vec![default_path.to_string_lossy().to_string()]);
        path_txt.move_cursor(CursorMove::End);

        let paragraph_text = format!("Journal: {}", entry.title.to_owned());

        let mut export_popup = ExportPopup {
            path_txt,
            path_err_msg: String::default(),
            entry_id: Some(entry.id),
            paragraph_text,
            replacement: None,
        };

        export_popup.validate_path();

        Ok(export_popup)
    }

    pub fn create_multi_select<D: DataProvider>(app: &App<D>) -> anyhow::Result<Self> {
        let mut default_path = if let Some(path) = &app.settings.export.default_path {
            path.clone()
        } else {
            env::current_dir()?
        };

        if default_path.is_dir() || has_trailing_separator(&default_path) {
            default_path.push(DEFAULT_FILE_NAME);
        }

        let mut path_txt = TextArea::new(vec![default_path.to_string_lossy().to_string()]);
        path_txt.move_cursor(CursorMove::End);

        let paragraph_text = format!(
            "Export the selected {} journals",
            app.selected_entries.len()
        );

        let mut export_popup = ExportPopup {
            path_txt,
            path_err_msg: String::default(),
            entry_id: None,
            paragraph_text,
            replacement: None,
        };

        export_popup.validate_path();

        Ok(export_popup)
    }

    fn validate_path(&mut self) {
        let path = self
            .path_txt
            .lines()
            .first()
            .expect("Path Textbox should always have one line");

        if path.is_empty() {
            self.path_err_msg = "Path can't be empty".into();
        } else {
            self.path_err_msg.clear();
        }
    }

    fn is_input_valid(&self) -> bool {
        self.path_err_msg.is_empty()
    }

    fn is_multi_select_mode(&self) -> bool {
        self.entry_id.is_none()
    }

    pub fn render_widget(&mut self, frame: &mut Frame, area: Rect, styles: &Styles) {
        let bounds = area;
        let mut area = centered_rect_exact_height(70, 11, area);

        if area.width < FOOTER_TEXT.len() as u16 + FOOTER_MARGINE {
            area.height += 1;
        }
        area = area.intersection(bounds);

        let title = if self.is_multi_select_mode() {
            "Export journals"
        } else {
            "Export journal content"
        };

        let block = Block::default().borders(Borders::ALL).title(title);

        frame.render_widget(Clear, area);
        frame.render_widget(block, area);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .horizontal_margin(4)
            .vertical_margin(2)
            .constraints([
                Constraint::Length(2),
                Constraint::Length(3),
                Constraint::Length(1),
                Constraint::Min(1),
            ])
            .split(area);

        let message = if self.replacement.is_some() {
            "This file already exists. Replace it?"
        } else {
            self.paragraph_text.as_str()
        };
        let journal_paragraph = Paragraph::new(message).wrap(Wrap { trim: false });
        frame.render_widget(journal_paragraph, chunks[0]);

        if self.path_err_msg.is_empty() {
            let block = Style::from(styles.general.input_block_active);
            let cursor = Style::from(styles.general.input_cursor_active);
            self.path_txt.set_style(block);
            self.path_txt.set_cursor_style(cursor);
            self.path_txt.set_block(
                Block::default()
                    .borders(Borders::ALL)
                    .style(block)
                    .title("Path"),
            );
        } else {
            let block = Style::from(styles.general.input_block_invalid);
            let cursor = Style::from(styles.general.input_cursor_invalid);
            self.path_txt.set_style(block);
            self.path_txt.set_cursor_style(cursor);
            self.path_txt.set_block(
                Block::default()
                    .borders(Borders::ALL)
                    .style(block)
                    .title(format!("Path : {}", self.path_err_msg)),
            );
        }

        self.path_txt.set_cursor_line_style(Style::default());

        frame.render_widget(&self.path_txt, chunks[1]);

        let footer_text = if self.replacement.is_some() {
            "y: replace file | n or Esc: go back"
        } else {
            FOOTER_TEXT
        };
        let footer = Paragraph::new(footer_text)
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: false });

        frame.render_widget(footer, chunks[3]);
    }

    pub fn handle_input(&mut self, input: &Input) -> ExportPopupInputReturn {
        if self.replacement.is_some() {
            return match input.key_code {
                KeyCode::Char('y') if input.modifiers.is_empty() => {
                    ExportPopupInputReturn::Apply((self.replacement.take().unwrap(), self.entry_id))
                }
                KeyCode::Char('n') | KeyCode::Esc => {
                    self.replacement = None;
                    ExportPopupInputReturn::KeepPopup
                }
                KeyCode::Char('c') if input.modifiers.contains(KeyModifiers::CONTROL) => {
                    ExportPopupInputReturn::Cancel
                }
                _ => ExportPopupInputReturn::KeepPopup,
            };
        }
        let has_ctrl = input.modifiers.contains(KeyModifiers::CONTROL);
        match input.key_code {
            KeyCode::Esc => ExportPopupInputReturn::Cancel,
            KeyCode::Char('c') if has_ctrl => ExportPopupInputReturn::Cancel,
            KeyCode::Enter => self.handle_confirm(),
            _ => {
                if self.path_txt.input(KeyEvent::from(input)) {
                    self.validate_path();
                }
                ExportPopupInputReturn::KeepPopup
            }
        }
    }

    fn handle_confirm(&mut self) -> ExportPopupInputReturn {
        self.validate_path();
        if !self.is_input_valid() {
            return ExportPopupInputReturn::KeepPopup;
        }

        let path: PathBuf = self
            .path_txt
            .lines()
            .first()
            .expect("Path Textbox should always have one line")
            .parse()
            .expect("PathBuf from string should never fail");

        match ExportTarget::inspect(path) {
            Ok(target) if target.replaces_file() => {
                self.replacement = Some(target);
                ExportPopupInputReturn::KeepPopup
            }
            Ok(target) => ExportPopupInputReturn::Apply((target, self.entry_id)),
            Err(error) => {
                self.path_err_msg = error.to_string();
                ExportPopupInputReturn::KeepPopup
            }
        }
    }
}

fn has_trailing_separator(path: &std::path::Path) -> bool {
    let text = path.as_os_str().to_string_lossy();
    text.ends_with(std::path::MAIN_SEPARATOR)
}

fn suggested_filename(entry: &Entry) -> String {
    let mut name = String::new();
    for ch in entry.title.chars() {
        let ch = if ch.is_control() || matches!(ch, '/' | '\\' | ':') {
            '_'
        } else {
            ch
        };
        if name.len() + ch.len_utf8() > 180 {
            break;
        }
        name.push(ch);
    }
    let name = name.trim_matches([' ', '.']);
    if name.is_empty() {
        format!("entry_{}.txt", entry.id)
    } else {
        format!("{name}.txt")
    }
}

#[cfg(test)]
mod tests;

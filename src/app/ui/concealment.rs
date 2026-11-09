use backend::DataProvider;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Wrap},
};

use super::{
    App, ControlType, HandleInputReturnType, Input, MsgBox, MsgBoxActions, MsgBoxResult,
    MsgBoxType, UICommand, UIComponents, divider::PaneSplit, msg_box::MsgBoxInputResult,
    ui_functions::menu_key_style,
};

enum ExitStep {
    DiscardDialog,
    SaveDraft,
    SaveError,
}

pub(super) struct ConcealedExit {
    step: ExitStep,
    popup: MsgBox,
}

impl ConcealedExit {
    fn new(step: ExitStep) -> Self {
        let (text, actions) = match step {
            ExitStep::DiscardDialog => (
                "Discard open dialog changes and leave?",
                MsgBoxActions::YesNo,
            ),
            ExitStep::SaveDraft => (
                "Save the current entry before leaving? No discards its changes.",
                MsgBoxActions::YesNoCancel,
            ),
            ExitStep::SaveError => (
                "Could not save. Your draft is kept. Reveal details to inspect it, or retry leaving.",
                MsgBoxActions::Ok,
            ),
        };
        Self {
            step,
            popup: MsgBox::new(MsgBoxType::Question(text.into()), actions),
        }
    }
}

impl UIComponents<'_> {
    pub fn is_details_hidden(&self) -> bool {
        self.details_hidden
    }

    pub(super) fn toggle_details_hidden<D: DataProvider>(&mut self, app: &mut App<D>) {
        if !app.journal_is_protected() || self.privacy_shield {
            return;
        }
        self.details_hidden = !self.details_hidden;
        self.concealed_exit = None;
        self.pane_split = None;
        self.cancel_divider_drag();
        app.redraw_after_restore = true;
    }

    pub(super) fn render_concealed<D: DataProvider>(&mut self, frame: &mut Frame, app: &App<D>) {
        self.pane_split = None;
        frame.render_widget(Clear, frame.area());
        let footer = Paragraph::new(Line::from(vec![
            Span::raw("Show details: "),
            Span::styled("<Ctrl-o>", menu_key_style()),
            Span::raw(" | Leave journal: "),
            Span::styled("q", menu_key_style()),
        ]))
        .wrap(Wrap { trim: false });
        let footer_height = footer
            .line_count(frame.area().width.max(1))
            .min(u16::MAX as usize) as u16;
        let chunks = Layout::vertical([Constraint::Min(0), Constraint::Length(footer_height)])
            .split(frame.area());
        let entries_style = self.styles.journals_list.block_inactive.into();
        let content_style = self.styles.editor.block_normal_inactive.into();
        if app.state.full_screen {
            let (title, style) = match self.active_control {
                ControlType::EntriesList => ("Entries — hidden", entries_style),
                ControlType::EntryContentTxt => ("Content — hidden", content_style),
            };
            render_scrambled(frame, chunks[0], title, style);
        } else if let Some(split) = PaneSplit::new(chunks[0], app.state.entries_list_percentage()) {
            render_scrambled(frame, split.list, "Entries — hidden", entries_style);
            render_scrambled(frame, split.editor, "Content — hidden", content_style);
        } else {
            frame.render_widget(Paragraph::new("Details hidden"), chunks[0]);
        }
        frame.render_widget(footer, chunks[1]);
        if let Some(exit) = &mut self.concealed_exit {
            exit.popup.render_widget(frame, frame.area(), &self.styles);
        }
    }

    fn request_concealed_exit(&mut self) -> HandleInputReturnType {
        if self.has_unsaved() {
            self.concealed_exit = Some(ConcealedExit::new(ExitStep::SaveDraft));
            HandleInputReturnType::Handled
        } else {
            HandleInputReturnType::ExitApp
        }
    }

    pub(super) async fn handle_concealed_input<D: DataProvider>(
        &mut self,
        input: &Input,
        app: &mut App<D>,
    ) -> anyhow::Result<HandleInputReturnType> {
        if !input.modifiers.is_empty() {
            return Ok(HandleInputReturnType::Handled);
        }
        if let Some(exit) = &self.concealed_exit {
            let MsgBoxInputResult::Close(result) = exit.popup.handle_input(input) else {
                return Ok(HandleInputReturnType::Handled);
            };
            let exit = self.concealed_exit.take().expect("exit prompt is present");
            match exit.step {
                ExitStep::DiscardDialog if result == MsgBoxResult::Yes => {
                    return Ok(self.request_concealed_exit());
                }
                ExitStep::SaveDraft if matches!(result, MsgBoxResult::Yes | MsgBoxResult::No) => {
                    return match UICommand::Quit.continue_executing(self, app, result).await {
                        Ok(result) => Ok(result),
                        Err(_) => {
                            self.concealed_exit = Some(ConcealedExit::new(ExitStep::SaveError));
                            Ok(HandleInputReturnType::Handled)
                        }
                    };
                }
                _ => {}
            }
        } else if *input == Input::new(KeyCode::Char('q'), KeyModifiers::NONE) {
            if self.has_popup() {
                self.concealed_exit = Some(ConcealedExit::new(ExitStep::DiscardDialog));
            } else {
                return Ok(self.request_concealed_exit());
            }
        }
        Ok(HandleInputReturnType::Handled)
    }
}

fn render_scrambled(frame: &mut Frame, area: Rect, title: &'static str, style: Style) {
    let block = Block::bordered().title(title).style(style);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width < 3 {
        return;
    }
    // These placeholders depend only on the viewport, never on journal text or entry count.
    const SCRAMBLE: &[u8] = b"x7q%R2#nK9@p4W&z8sT3*m6";
    for row in 0..inner.height {
        if row % 4 == 3 {
            continue;
        }
        let width = inner.width.saturating_sub(2).min(48);
        let text: String = (0..width)
            .map(|column| {
                char::from(SCRAMBLE[(usize::from(row) * 7 + usize::from(column)) % SCRAMBLE.len()])
            })
            .collect();
        frame.render_widget(
            Paragraph::new(text).style(style),
            Rect::new(inner.x.saturating_add(1), inner.y + row, width, 1),
        );
    }
}

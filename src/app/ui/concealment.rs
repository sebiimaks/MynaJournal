use backend::DataProvider;
use crossterm::event::{KeyCode, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Wrap},
};

use super::{
    App, HandleInputReturnType, Input, MsgBox, MsgBoxActions, MsgBoxResult, MsgBoxType, UICommand,
    UIComponents, msg_box::MsgBoxInputResult, text_mask::TextMask, ui_functions::menu_key_style,
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
        self.text_mask = self.details_hidden.then(TextMask::new);
        self.editor.set_text_mask(self.text_mask);
        self.entries_list.set_text_mask(self.text_mask);
        self.concealed_exit = None;
        self.pane_split = None;
        self.cancel_divider_drag();
        app.redraw_after_restore = true;
    }

    pub(crate) fn conceal_active_word(&mut self) -> bool {
        self.editor.clear_visible_word()
    }

    pub(super) fn render_concealed_popup(&mut self, frame: &mut Frame) {
        if let Some(exit) = &mut self.concealed_exit {
            exit.popup.render_widget(frame, frame.area(), &self.styles);
        } else if self.has_popup() {
            let bounds = frame.area();
            let width = bounds.width.min(58);
            let paragraph = Paragraph::new(vec![
                Line::raw("Show details to resume the open dialog."),
                Line::default(),
                Line::from(vec![
                    Span::raw("Show details: "),
                    Span::styled("<Ctrl-o>", menu_key_style()),
                    Span::raw(" | Leave journal: "),
                    Span::styled("q", menu_key_style()),
                ]),
            ])
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: false });
            let height = paragraph
                .line_count(width.saturating_sub(2))
                .saturating_add(2)
                .min(usize::from(bounds.height)) as u16;
            let area = Rect::new(
                bounds.x + (bounds.width - width) / 2,
                bounds.y + (bounds.height - height) / 2,
                width,
                height,
            );
            frame.render_widget(Clear, area);
            frame.render_widget(
                paragraph.block(
                    Block::bordered()
                        .title("Dialog hidden")
                        .border_style(Style::from(self.styles.general.input_block_active)),
                ),
                area,
            );
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
        self.editor.clear_visible_word();
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

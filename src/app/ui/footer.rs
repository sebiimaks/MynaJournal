use backend::DataProvider;
use ratatui::{
    Frame,
    layout::Rect,
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};

use crate::app::{App, keymap::Keymap};

use super::{ControlType, UICommand, UIComponents, ui_functions::menu_key_style};

const SEPARATOR: &str = " | ";

pub fn get_footer_height<D: DataProvider>(
    width: u16,
    ui_components: &UIComponents,
    app: &App<D>,
) -> u16 {
    footer(ui_components, app)
        .line_count(width)
        .min(u16::MAX as usize) as u16
}

pub fn render_footer<D: DataProvider>(
    frame: &mut Frame,
    area: Rect,
    ui_components: &UIComponents,
    app: &App<D>,
) {
    frame.render_widget(footer(ui_components, app), area);
}

fn footer<D: DataProvider>(ui: &UIComponents, app: &App<D>) -> Paragraph<'static> {
    let text = match (
        ui.editor.is_insert_mode(),
        ui.entries_list.multi_select_mode,
    ) {
        (true, false) => {
            let command = UICommand::BackEditorNormalMode;
            let mut line = command_text(&command.get_info().name, &ui.editor_keymaps, command);
            line.push_span(Span::raw(SEPARATOR));
            line.push_span(Span::raw("Edit using Emacs motions"));
            line
        }
        (false, true) => {
            let command = UICommand::LeaveMultiSelectMode;
            join_parts(vec![
                command_text(&command.get_info().name, &ui.multi_select_keymaps, command),
                command_text("Help", &ui.multi_select_keymaps, UICommand::ShowHelp),
                command_text("Config", &ui.multi_select_keymaps, UICommand::ShowConfig),
            ])
        }
        _ => standard_text(ui, app),
    };
    Paragraph::new(text).wrap(Wrap { trim: false })
}

fn standard_text<D: DataProvider>(ui: &UIComponents, app: &App<D>) -> Line<'static> {
    let mut parts = vec![
        command_text("Exit", &ui.global_keymaps, UICommand::Quit),
        command_text(
            "Edit content",
            &ui.global_keymaps,
            UICommand::StartEditEntryContent,
        ),
        command_text(
            "Entry security",
            &ui.global_keymaps,
            UICommand::ShowSecurity,
        ),
    ];
    if ui.active_control == ControlType::EntriesList {
        let mut filter = command_text(
            "Filter open",
            &ui.entries_list_keymaps,
            UICommand::ShowFilter,
        );
        filter.push_span(Span::raw("  "));
        filter
            .spans
            .extend(command_text("reset", &ui.entries_list_keymaps, UICommand::ResetFilter).spans);
        parts.push(filter);
        parts.push(command_text(
            "Sort options",
            &ui.entries_list_keymaps,
            UICommand::ShowSortOptions,
        ));
    }
    if app.state.full_screen {
        let command = UICommand::ToggleFullScreenMode;
        parts.push(command_text(
            &command.get_info().name,
            &ui.global_keymaps,
            command,
        ));
    }
    parts.push(command_text(
        "Help",
        &ui.global_keymaps,
        UICommand::ShowHelp,
    ));
    parts.push(command_text(
        "Config",
        &ui.global_keymaps,
        UICommand::ShowConfig,
    ));
    if app.has_sealed_drafts() {
        parts.push(Line::raw("Encrypted unsaved draft: unlock to save"));
    }
    join_parts(parts)
}

fn command_text(label: &str, keymaps: &[Keymap], command: UICommand) -> Line<'static> {
    let mut spans = vec![Span::raw(format!("{label}: "))];
    for (index, keymap) in keymaps
        .iter()
        .filter(|keymap| keymap.command == command)
        .filter(|keymap| {
            !matches!(command, UICommand::ShowConfig | UICommand::ShowSecurity)
                || keymap.key.modifiers.is_empty()
        })
        .enumerate()
    {
        if index > 0 {
            spans.push(Span::raw(", "));
        }
        spans.push(Span::styled(keymap.key.to_string(), menu_key_style()));
    }
    Line::from(spans)
}

fn join_parts(parts: Vec<Line<'static>>) -> Line<'static> {
    let mut line = Line::default();
    for (index, part) in parts.into_iter().enumerate() {
        if index > 0 {
            line.push_span(Span::raw(SEPARATOR));
        }
        line.spans.extend(part.spans);
    }
    line
}

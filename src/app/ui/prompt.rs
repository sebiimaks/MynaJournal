use backend::{DataProvider, JournalPrompt};
use ratatui::{
    Frame,
    layout::{Margin, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Paragraph, Wrap},
};

use super::{App, UIComponents, ui_functions::menu_key_style};

impl UIComponents<'_> {
    pub(super) fn current_prompt<'a, D: DataProvider>(
        &self,
        app: &'a App<D>,
    ) -> Option<&'a JournalPrompt> {
        if !app.settings.show_entry_mood
            || !app.settings.journal_prompts
            || self.startup_artwork_visible
            || self.privacy_shield
            || app.current_is_locked()
        {
            return None;
        }
        app.get_current_entry()?.journal_prompt.as_ref()
    }

    pub(super) fn render_editor_with_prompt<D: DataProvider>(
        &mut self,
        frame: &mut Frame,
        area: Rect,
        app: &App<D>,
    ) {
        let prompt = self.current_prompt(app).filter(|_| !self.prompt_hidden);
        let layout = prompt.and_then(|prompt| PromptLayout::new(area, &prompt.text));
        self.editor.render_widget(
            frame,
            area,
            &self.styles,
            layout.as_ref().map_or(0, |layout| layout.top_inset),
        );
        let Some((prompt, layout)) = prompt.zip(layout) else {
            return;
        };
        let title = if layout.truncated {
            Line::raw("Prompt — enlarge pane to read all")
        } else {
            Line::from(vec![
                Span::raw("Journal prompt | "),
                Span::styled("Ctrl-G", menu_key_style()),
            ])
        };
        let block = Block::bordered()
            .title(title)
            .style(Style::from(self.styles.editor.block_normal_inactive));
        frame.render_widget(
            Paragraph::new(prompt.text.as_str())
                .wrap(Wrap { trim: false })
                .block(block),
            layout.area,
        );
    }
}

struct PromptLayout {
    area: Rect,
    top_inset: u16,
    truncated: bool,
}

impl PromptLayout {
    fn new(content: Rect, text: &str) -> Option<Self> {
        let inner = content.inner(Margin::new(1, 1));
        let width = (u32::from(inner.width) * 60 / 100) as u16;
        // Leave a row above and below the prompt, and at least three editable rows.
        let maximum_height = inner.height.saturating_sub(5);
        if width < 4 || maximum_height < 3 {
            return None;
        }
        let needed = Paragraph::new(text)
            .wrap(Wrap { trim: false })
            .line_count(width - 2)
            .saturating_add(2);
        let height = needed.min(usize::from(maximum_height)) as u16;
        Some(Self {
            area: Rect::new(
                inner.x + (inner.width - width) / 2,
                inner.y + 1,
                width,
                height,
            ),
            top_inset: height + 2,
            truncated: needed > usize::from(height),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{
        keymap::Input,
        test::create_default_app,
        ui::{ControlType, Styles, UICommand},
    };
    use backend::Mood;
    use chrono::Utc;
    use crossterm::event::{KeyCode, KeyModifiers};
    use ratatui::{Terminal, backend::TestBackend};

    fn screen<D: DataProvider>(
        ui: &mut UIComponents<'_>,
        app: &App<D>,
        width: u16,
        height: u16,
    ) -> String {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| ui.render_ui(frame, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[tokio::test]
    async fn prompt_toggle_works_while_editing_without_changing_content_or_saved_prompt() {
        let mut app = create_default_app();
        app.load_entries().await.unwrap();
        let id = app
            .add_entry(
                "Reflection".into(),
                Utc::now(),
                vec![],
                None,
                Some(Mood::Average),
                true,
            )
            .await
            .unwrap();
        let original = app.get_entry(id).unwrap().clone();
        let mut ui = UIComponents::new(Styles::default());
        ui.set_current_entry(Some(id), &mut app);
        assert!(screen(&mut ui, &app, 100, 30).contains("Journal prompt"));
        ui.change_active_control(ControlType::EntryContentTxt);
        UICommand::StartEditEntryContent
            .execute(&mut ui, &mut app)
            .await
            .unwrap();
        ui.editor.set_entry_content("Unsaved body", &app);
        let toggle = Input::new(KeyCode::Char('g'), KeyModifiers::CONTROL);
        ui.handle_input(&toggle, &mut app).await.unwrap();
        assert!(!screen(&mut ui, &app, 100, 30).contains("Journal prompt"));
        assert_eq!(ui.editor.get_content(), "Unsaved body");
        assert_eq!(app.get_entry(id), Some(&original));
        ui.handle_input(&toggle, &mut app).await.unwrap();
        assert!(screen(&mut ui, &app, 100, 30).contains("Journal prompt"));
        assert_eq!(ui.editor.get_content(), "Unsaved body");
        assert_eq!(app.get_entry(id), Some(&original));
        ui.handle_input(&toggle, &mut app).await.unwrap();
        ui.set_current_entry(Some(id), &mut app);
        assert!(ui.prompt_hidden);
        ui.set_current_entry(Some(0), &mut app);
        ui.set_current_entry(Some(id), &mut app);
        assert!(!ui.prompt_hidden);
    }

    #[tokio::test]
    async fn prompt_visibility_respects_settings_startup_and_note_locking() {
        let mut app = create_default_app();
        let id = app
            .add_entry(
                "Reflection".into(),
                Utc::now(),
                vec![],
                None,
                Some(Mood::Low),
                true,
            )
            .await
            .unwrap();
        let mut ui = UIComponents::new(Styles::default());
        ui.set_current_entry(Some(id), &mut app);
        for (mood, prompts, expected) in [
            (false, true, false),
            (true, false, false),
            (true, true, true),
        ] {
            let settings = crate::settings::Settings {
                show_entry_mood: mood,
                journal_prompts: prompts,
                ..app.settings.clone()
            };
            app.apply_live_settings(settings);
            assert_eq!(ui.current_prompt(&app).is_some(), expected);
            assert_eq!(
                screen(&mut ui, &app, 100, 30).contains("Journal prompt"),
                expected
            );
        }
        ui.show_startup_artwork();
        assert!(ui.current_prompt(&app).is_none());
        ui.dismiss_startup_artwork();
        let password = backend::protection::SecretString::from("prompt-ui-test".to_owned());
        app.protect_note(id, password.clone(), "Body".into())
            .await
            .unwrap();
        ui.set_current_entry(Some(id), &mut app);
        let locked = screen(&mut ui, &app, 100, 30);
        assert!(!locked.contains("Journal prompt"));
        assert!(!locked.contains("[-]"));
        app.unlock_note(id, password).await.unwrap();
        ui.set_current_entry(Some(id), &mut app);
        let unlocked = screen(&mut ui, &app, 100, 30);
        assert!(unlocked.contains("Journal prompt"));
        assert!(unlocked.contains("[-]"));
        ui.hide_for_auto_lock();
        assert!(ui.current_prompt(&app).is_none());
        assert!(!screen(&mut ui, &app, 100, 30).contains("Journal prompt"));
    }

    #[test]
    fn prompt_layout_is_centred_inside_the_top_of_the_content_border() {
        for area in [
            Rect::new(17, 4, 101, 30),
            Rect::new(0, 0, 80, 24),
            Rect::new(6, 2, 24, 10),
        ] {
            let layout = PromptLayout::new(area, "A journal prompt").unwrap();
            let inner = area.inner(Margin::new(1, 1));
            assert_eq!(
                layout.area.width,
                (u32::from(inner.width) * 60 / 100) as u16
            );
            let left = layout.area.x - inner.x;
            let right = inner.right() - layout.area.right();
            assert!(left.abs_diff(right) <= 1);
            assert_eq!(layout.area.y, inner.y + 1);
            assert_eq!(inner.y + layout.top_inset, layout.area.bottom() + 1);
            assert!(inner.height - layout.top_inset >= 3);
        }
        for size in [(10, 7), (8, 5), (1, 1)] {
            assert!(PromptLayout::new(Rect::new(0, 0, size.0, size.1), "Prompt").is_none());
        }
    }

    #[tokio::test]
    async fn prompt_and_body_share_one_content_border_with_and_without_word_wrap() {
        let mut app = create_default_app();
        let id = app
            .add_entry(
                "Reflection".into(),
                Utc::now(),
                vec![],
                None,
                Some(Mood::High),
                true,
            )
            .await
            .unwrap();
        let mut ui = UIComponents::new(Styles::default());
        ui.set_current_entry(Some(id), &mut app);
        ui.editor.set_entry_content("Body starts here", &app);
        for word_wrap in [false, true] {
            ui.editor.set_word_wrap(word_wrap);
            for (width, height) in [(100, 30), (80, 24), (24, 10), (10, 7), (1, 1)] {
                let area = Rect::new(0, 0, width, height);
                let layout = PromptLayout::new(
                    area,
                    &app.get_current_entry()
                        .unwrap()
                        .journal_prompt
                        .as_ref()
                        .unwrap()
                        .text,
                );
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal
                    .draw(|frame| ui.render_note_pane(frame, area, &app))
                    .unwrap();
                let buffer = terminal.backend().buffer();
                if width >= 24 {
                    assert_eq!(buffer[(0, 0)].symbol(), "┌");
                    assert_eq!(buffer[(width - 1, height - 1)].symbol(), "┘");
                    for y in 1..height - 1 {
                        assert_eq!(buffer[(0, y)].symbol(), "│");
                        assert_eq!(buffer[(width - 1, y)].symbol(), "│");
                    }
                    if let Some(layout) = layout {
                        assert_eq!(buffer[(layout.area.x, layout.area.y)].symbol(), "┌");
                        assert_eq!(
                            buffer[(layout.area.right() - 1, layout.area.bottom() - 1)].symbol(),
                            "┘"
                        );
                        assert_eq!(buffer[(1, layout.area.bottom() + 1)].symbol(), "B");
                    }
                }
                assert_eq!(ui.editor.get_content(), "Body starts here");
            }
        }
    }
}

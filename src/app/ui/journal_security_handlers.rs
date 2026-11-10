use anyhow::{Result, ensure};
use backend::DataProvider;

use super::journal_security_popup::JournalSecurityAction;
use super::{App, ControlType, Editor, HandleInputReturnType, Popup, UIComponents};
use crate::app::keymap::Input;

impl UIComponents<'_> {
    pub(super) async fn handle_journal_security_input<D: DataProvider>(
        &mut self,
        input: &Input,
        app: &mut App<D>,
    ) -> Result<HandleInputReturnType> {
        let action = match self.popup_stack.last_mut() {
            Some(Popup::JournalSecurity(popup)) => popup.handle_input(input),
            _ => return Ok(HandleInputReturnType::Handled),
        };
        match action {
            JournalSecurityAction::Keep => return Ok(HandleInputReturnType::Handled),
            JournalSecurityAction::Close => {
                self.cancel_journal_consolidation(app);
                self.popup_stack.pop();
                return Ok(HandleInputReturnType::Handled);
            }
            _ => {}
        }
        let refresh_editor = !matches!(action, JournalSecurityAction::Change { .. });
        let was_protected = app.journal_is_protected();
        let mut converting = false;
        let result: Result<bool> = async {
            ensure!(
                !self.has_unsaved()
                    && !self
                        .popup_stack
                        .iter()
                        .any(|popup| matches!(popup, Popup::Entry(_))),
                "Save or discard entry edits before changing journal protection"
            );
            match action {
                JournalSecurityAction::Enable(password) => {
                    ensure!(!app.has_sealed_drafts(), "Unlock and save or discard all encrypted entry drafts before protecting the journal");
                    let locked = app.locked_note_ids();
                    if !locked.is_empty() {
                        if let Some(Popup::JournalSecurity(popup)) = self.popup_stack.last_mut() {
                            popup.begin_consolidation(password, locked);
                        }
                        return Ok(false);
                    }
                    converting = true;
                    app.enable_journal_protection(password).await?;
                }
                JournalSecurityAction::UnlockEntry { id, password } => {
                    ensure!(matches!(self.popup_stack.last(), Some(Popup::JournalSecurity(popup)) if popup.consolidating_entries()), "Journal protection setup is no longer active");
                    ensure!(!app.has_sealed_drafts(), "Unlock and save or discard all encrypted entry drafts before protecting the journal");
                    app.unlock_note(id, password).await?;
                    let password = match self.popup_stack.last_mut() {
                        Some(Popup::JournalSecurity(popup)) => popup.entry_unlocked(id),
                        _ => unreachable!(),
                    };
                    let Some(password) = password else { return Ok(false) };
                    converting = true;
                    app.enable_journal_protection(password).await?;
                }
                JournalSecurityAction::Change { current, new } => {
                    app.change_journal_password(current, new).await?
                }
                JournalSecurityAction::Disable(password) => {
                    app.disable_journal_protection(password).await?
                }
                JournalSecurityAction::Unlock(_) => {
                    anyhow::bail!("Open locked journals from the journal selection screen")
                }
                JournalSecurityAction::Keep | JournalSecurityAction::Close => unreachable!(),
            }
            Ok(true)
        }
        .await;
        match result {
            Ok(true) => {
                self.popup_stack.pop();
                for popup in &mut self.popup_stack {
                    if let Popup::Config(popup) = popup {
                        popup.set_journal_protected(app.journal_is_protected());
                    }
                }
                if refresh_editor {
                    self.editor = Editor::new();
                    self.editor
                        .set_active(self.active_control == ControlType::EntryContentTxt);
                    self.editor.set_current_entry(app.current_entry_id, app);
                }
            }
            Ok(false) => {}
            Err(error) => {
                let conversion_failed = converting
                    || app.data_provide.journal_reload_required()
                    || app.journal_is_protected() != was_protected;
                if converting {
                    self.cancel_journal_consolidation(app);
                }
                if app.journal_is_protected() != was_protected
                    || (refresh_editor && app.current_entry_id.is_none())
                {
                    for popup in &mut self.popup_stack {
                        if let Popup::Config(popup) = popup {
                            popup.set_journal_protected(app.journal_is_protected());
                        }
                    }
                    self.editor = Editor::new();
                    self.editor
                        .set_active(self.active_control == ControlType::EntryContentTxt);
                    self.editor.set_current_entry(app.current_entry_id, app);
                }
                if let Some(Popup::JournalSecurity(popup)) = self.popup_stack.last_mut() {
                    if conversion_failed {
                        popup.show_failure(format!("{error:#}"));
                    } else {
                        popup.show_error(error.to_string());
                    }
                } else {
                    return Err(error);
                }
            }
        }
        if let Some(Popup::JournalSecurity(popup)) = self.popup_stack.last_mut()
            && let Some(id) = popup.entry_to_unlock()
        {
            popup.set_entry_title(app.get_entry(id).map_or("", |entry| entry.title.as_str()));
        }
        Ok(HandleInputReturnType::Handled)
    }

    fn cancel_journal_consolidation<D: DataProvider>(&mut self, app: &mut App<D>) {
        if let Some(Popup::JournalSecurity(popup)) = self.popup_stack.last_mut()
            && popup.consolidating_entries()
        {
            for id in popup.cancel_consolidation() {
                app.lock_note(id);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{
        test::create_default_app,
        ui::{
            Styles,
            journal_security_popup::{JournalPasswordAction, JournalSecurityPopup},
        },
    };
    use crossterm::event::{KeyCode, KeyModifiers};

    async fn enter_password<D: DataProvider>(ui: &mut UIComponents<'_>, app: &mut App<D>) {
        enter_text(ui, app, "entry test password").await;
    }

    async fn enter_text<D: DataProvider>(ui: &mut UIComponents<'_>, app: &mut App<D>, text: &str) {
        for ch in text.chars() {
            ui.handle_journal_security_input(
                &Input::new(KeyCode::Char(ch), KeyModifiers::NONE),
                app,
            )
            .await
            .unwrap();
        }
        ui.handle_journal_security_input(&Input::new(KeyCode::Enter, KeyModifiers::NONE), app)
            .await
            .unwrap();
    }

    fn assert_editor_focused(ui: &mut UIComponents<'_>) {
        use ratatui::{Terminal, backend::TestBackend};
        assert_eq!(ui.active_control, ControlType::EntryContentTxt);
        let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
        terminal
            .draw(|frame| {
                let area = frame.area();
                ui.editor.render_widget(frame, area, &ui.styles, 0);
            })
            .unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("Content - NORMAL"));
    }

    #[cfg(feature = "file")]
    #[tokio::test]
    async fn blocked_file_conversion_shows_the_cause_and_closes_to_config_without_retrying() {
        use crate::{
            app::ui::{ConfigPopup, themes::ThemeStore},
            settings::document::ConfigDocument,
        };
        use backend::{
            EntryDraft, FileFormat,
            journal::{JournalBackend, JournalProvider},
        };
        use ratatui::{Terminal, backend::TestBackend};

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("entries");
        let provider = JournalProvider::open(JournalBackend::File(path.clone(), FileFormat::Plain))
            .await
            .unwrap();
        let mut app = App::new(provider, crate::settings::Settings::default());
        app.data_provide
            .add_entry(EntryDraft::new(
                chrono::Utc::now(),
                "Existing entry".into(),
                vec![],
                None,
            ))
            .await
            .unwrap();
        app.load_entries().await.unwrap();
        let unrelated = path.join(".sync.ffs_db");
        std::fs::write(&unrelated, "Keep this file").unwrap();
        let mut ui = UIComponents::new(Styles::default());
        let config = ConfigPopup::new(
            ConfigDocument::load(directory.path().join("config.toml")).unwrap(),
            ThemeStore::from_directory(directory.path().join("themes")).unwrap(),
            &app.settings,
            &Styles::default(),
            None,
            30,
        )
        .unwrap();
        ui.popup_stack.push(Popup::Config(Box::new(config)));
        ui.popup_stack
            .push(Popup::JournalSecurity(Box::new(JournalSecurityPopup::new(
                JournalPasswordAction::Enable,
            ))));
        ui.handle_journal_security_input(
            &Input::new(KeyCode::Char('y'), KeyModifiers::NONE),
            &mut app,
        )
        .await
        .unwrap();
        enter_text(&mut ui, &mut app, "journal secret").await;
        enter_text(&mut ui, &mut app, "journal secret").await;
        assert!(!app.journal_is_protected());
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| ui.render_popup(frame)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("Journal Protection Error"));
        assert!(text.contains(".sync.ffs_db"), "{text}");
        assert!(!text.contains("New Password") && !text.contains("journal secret"));
        ui.handle_journal_security_input(
            &Input::new(KeyCode::Char('y'), KeyModifiers::NONE),
            &mut app,
        )
        .await
        .unwrap();
        ui.handle_journal_security_input(&Input::new(KeyCode::Enter, KeyModifiers::NONE), &mut app)
            .await
            .unwrap();
        assert_eq!(ui.popup_stack.len(), 1);
        assert!(matches!(ui.popup_stack.last(), Some(Popup::Config(_))));
        assert!(!app.journal_is_protected());
        assert_eq!(
            std::fs::read_to_string(unrelated).unwrap(),
            "Keep this file"
        );
    }

    #[cfg(feature = "json")]
    #[tokio::test]
    async fn journal_conversion_preserves_the_focused_content_pane() {
        use backend::{
            EntryDraft,
            journal::{JournalBackend, JournalProvider},
        };
        let directory = tempfile::tempdir().unwrap();
        let provider =
            JournalProvider::open(JournalBackend::Json(directory.path().join("entries.json")))
                .await
                .unwrap();
        let mut app = App::new(provider, crate::settings::Settings::default());
        app.data_provide
            .add_entry(
                EntryDraft::new(chrono::Utc::now(), "Title".into(), vec![], None)
                    .with_content("Content stays editable".into()),
            )
            .await
            .unwrap();
        app.load_entries().await.unwrap();
        let mut ui = UIComponents::new(Styles::default());
        ui.set_current_entry(app.entries.first().map(|entry| entry.id), &mut app);
        ui.change_active_control(ControlType::EntryContentTxt);
        for action in [
            JournalPasswordAction::Enable,
            JournalPasswordAction::Disable,
        ] {
            ui.popup_stack
                .push(Popup::JournalSecurity(Box::new(JournalSecurityPopup::new(
                    action,
                ))));
            ui.handle_journal_security_input(
                &Input::new(KeyCode::Char('y'), KeyModifiers::NONE),
                &mut app,
            )
            .await
            .unwrap();
            enter_text(&mut ui, &mut app, "journal test password").await;
            if action == JournalPasswordAction::Enable {
                enter_text(&mut ui, &mut app, "journal test password").await;
            }
            assert!(ui.popup_stack.is_empty());
            assert_eq!(
                app.journal_is_protected(),
                action == JournalPasswordAction::Enable
            );
            assert_editor_focused(&mut ui);
        }
    }

    #[cfg(feature = "json")]
    #[tokio::test]
    async fn confirmed_setup_consolidates_entry_passwords_and_reopens_with_journal_password() {
        use backend::{
            EntryDraft, JsonDataProvide,
            journal::{JournalBackend, JournalProvider},
        };
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("entries.json");
        let mut source = create_default_app();
        let mut native = JsonDataProvide::new(path.clone());
        for entry in source.data_provide.load_all_entries().await.unwrap() {
            native
                .add_entry(EntryDraft::from_entry(entry))
                .await
                .unwrap();
        }
        drop(native);
        let backend = JournalBackend::Json(path.clone());
        let provider = JournalProvider::open(backend.clone()).await.unwrap();
        let mut app = App::new(provider, crate::settings::Settings::default());
        app.load_entries().await.unwrap();
        for id in [0, 1] {
            app.protect_note(
                id,
                "entry test password".to_owned().into(),
                format!("secret body {id}"),
            )
            .await
            .unwrap();
        }
        let mut ui = UIComponents::new(Styles::default());
        ui.popup_stack
            .push(Popup::JournalSecurity(Box::new(JournalSecurityPopup::new(
                JournalPasswordAction::Enable,
            ))));
        ui.handle_journal_security_input(
            &Input::new(KeyCode::Char('y'), KeyModifiers::NONE),
            &mut app,
        )
        .await
        .unwrap();
        enter_text(&mut ui, &mut app, "journal test password").await;
        enter_text(&mut ui, &mut app, "journal test password").await;
        assert!(ui.journal_conversion_pending());
        enter_password(&mut ui, &mut app).await;
        assert!(ui.journal_conversion_pending());
        enter_password(&mut ui, &mut app).await;
        assert!(app.journal_is_protected());
        assert!(ui.popup_stack.is_empty());
        let contents = std::fs::read_to_string(path).unwrap();
        assert!(!contents.contains("secret body") && !contents.contains("Title 1"));
        drop(app);
        let mut reopened = JournalProvider::open(backend).await.unwrap();
        assert!(reopened.is_locked());
        assert!(
            reopened
                .unlock("entry test password".to_owned().into())
                .await
                .is_err()
        );
        reopened
            .unlock("journal test password".to_owned().into())
            .await
            .unwrap();
        let entries = reopened.load_all_entries().await.unwrap();
        assert_eq!(entries.len(), 2);
        assert!(entries.iter().all(|entry| entry.protection.is_none()));
        assert!(
            entries
                .iter()
                .any(|entry| entry.title == "Title 1" && entry.content == "secret body 0")
        );
        assert!(
            entries
                .iter()
                .any(|entry| entry.title == "Title 2" && entry.content == "secret body 1")
        );
    }

    #[tokio::test]
    async fn cancelling_conversion_relocks_entries_opened_during_setup_without_changing_storage() {
        let mut app = create_default_app();
        app.load_entries().await.unwrap();
        for id in [0, 1] {
            app.protect_note(
                id,
                "entry test password".to_owned().into(),
                format!("body {id}"),
            )
            .await
            .unwrap();
        }
        let original = app.data_provide.load_all_entries().await.unwrap();
        let mut ui = UIComponents::new(Styles::default());
        let mut popup = JournalSecurityPopup::new(JournalPasswordAction::Enable);
        popup.begin_consolidation("journal test password".to_owned().into(), vec![0, 1]);
        ui.popup_stack.push(Popup::JournalSecurity(Box::new(popup)));
        enter_password(&mut ui, &mut app).await;
        assert!(!app.is_locked(0) && app.is_locked(1));
        ui.hide_for_auto_lock();
        ui.auto_lock(&mut app).await.unwrap();
        assert!(!ui.is_privacy_shielded());
        assert!(!app.is_locked(0));
        ui.handle_journal_security_input(&Input::new(KeyCode::Esc, KeyModifiers::NONE), &mut app)
            .await
            .unwrap();
        assert!(app.is_locked(0) && app.is_locked(1));
        assert!(ui.popup_stack.is_empty());
        assert_eq!(app.data_provide.load_all_entries().await.unwrap(), original);
    }

    #[tokio::test]
    async fn failed_conversion_relocks_only_entries_opened_during_setup_and_retains_popup() {
        let mut app = create_default_app();
        app.load_entries().await.unwrap();
        for id in [0, 1] {
            app.protect_note(
                id,
                "entry test password".to_owned().into(),
                format!("body {id}"),
            )
            .await
            .unwrap();
        }
        app.unlock_note(0, "entry test password".to_owned().into())
            .await
            .unwrap();
        let mut ui = UIComponents::new(Styles::default());
        let mut popup = JournalSecurityPopup::new(JournalPasswordAction::Enable);
        popup.begin_consolidation("journal test password".to_owned().into(), vec![1]);
        ui.popup_stack.push(Popup::JournalSecurity(Box::new(popup)));
        app.data_provide.set_return_err(true);
        enter_password(&mut ui, &mut app).await;
        assert!(!app.is_locked(0) && app.is_locked(1));
        assert!(!app.journal_is_protected());
        assert!(
            matches!(ui.popup_stack.last(), Some(Popup::JournalSecurity(popup)) if !popup.consolidating_entries())
        );
    }

    #[tokio::test]
    async fn committed_protection_with_failed_reload_clears_stale_editor_and_updates_config_state()
    {
        use crate::{
            app::ui::{ConfigAction, ConfigPopup, themes::ThemeStore},
            settings::document::ConfigDocument,
        };
        let mut app = create_default_app();
        app.load_entries().await.unwrap();
        let mut ui = UIComponents::new(Styles::default());
        ui.set_current_entry(Some(0), &mut app);
        ui.change_active_control(ControlType::EntryContentTxt);
        assert!(!ui.editor.get_content().is_empty());
        let directory = tempfile::tempdir().unwrap();
        let mut config = ConfigPopup::new(
            ConfigDocument::load(directory.path().join("config.toml")).unwrap(),
            ThemeStore::from_directory(directory.path().join("themes")).unwrap(),
            &app.settings,
            &Styles::default(),
            None,
            30,
        )
        .unwrap();
        config
            .settings_document_mut()
            .set(&["colored_tags"], Some(false.into()))
            .unwrap();
        let draft = config.settings_document_mut().text();
        ui.popup_stack.push(Popup::Config(Box::new(config)));
        ui.popup_stack
            .push(Popup::JournalSecurity(Box::new(JournalSecurityPopup::new(
                JournalPasswordAction::Enable,
            ))));
        app.data_provide.commit_journal_conversion_then_fail_reload = true;
        ui.handle_journal_security_input(
            &Input::new(KeyCode::Char('y'), KeyModifiers::NONE),
            &mut app,
        )
        .await
        .unwrap();
        enter_text(&mut ui, &mut app, "journal password").await;
        enter_text(&mut ui, &mut app, "journal password").await;
        assert!(app.journal_is_protected());
        assert!(app.entries.is_empty() && app.current_entry_id.is_none());
        assert!(ui.editor.get_content().is_empty());
        assert!(matches!(
            ui.popup_stack.last(),
            Some(Popup::JournalSecurity(_))
        ));
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(80, 24)).unwrap();
        terminal.draw(|frame| ui.render_popup(frame)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("Journal Protection Error"));
        assert!(text.contains("Test Error"), "{text}");
        let Popup::Config(config) = &mut ui.popup_stack[0] else {
            panic!("Configuration draft was lost")
        };
        assert_eq!(config.settings_document_mut().text(), draft);
        for _ in 0..2 {
            config.handle_input(&Input::new(KeyCode::BackTab, KeyModifiers::NONE));
        }
        assert_eq!(
            config.handle_input(&Input::new(KeyCode::Enter, KeyModifiers::NONE)),
            ConfigAction::DisableJournalProtection
        );
        ui.handle_journal_security_input(&Input::new(KeyCode::Enter, KeyModifiers::NONE), &mut app)
            .await
            .unwrap();
        assert_eq!(ui.popup_stack.len(), 1);
        assert!(matches!(ui.popup_stack.last(), Some(Popup::Config(_))));
        ui.popup_stack.clear();
        assert_editor_focused(&mut ui);
    }
}

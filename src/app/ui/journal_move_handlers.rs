use anyhow::{Context, Result, ensure};
use backend::DataProvider;

use super::journal_move_popup::{MoveJournalAction, MoveJournalPopup};
use super::{App, HandleInputReturnType, Popup, UIComponents};
use crate::{app::keymap::Input, journal_move};

impl UIComponents<'_> {
    fn ensure_move_ready<D: DataProvider>(&self, app: &App<D>) -> Result<()> {
        ensure!(
            !self.has_unsaved()
                && !app.has_sealed_drafts()
                && !self
                    .popup_stack
                    .iter()
                    .any(|popup| matches!(popup, Popup::Entry(_))),
            "Save or discard all entry edits before moving this journal"
        );
        ensure!(
            !app.data_provide.journal_reload_required(),
            "Close and reopen this journal before moving it"
        );
        let context = app
            .config_context
            .as_ref()
            .context("Configuration paths are unavailable")?;
        ensure!(
            !context.storage_overridden,
            "Reopen without command-line storage overrides before moving this journal"
        );
        let config = self
            .popup_stack
            .iter()
            .find_map(|popup| match popup {
                Popup::Config(config) => Some(config),
                _ => None,
            })
            .context("Open Move journal from Configuration")?;
        ensure!(
            !config.is_dirty(),
            "Save or discard configuration changes before moving this journal"
        );
        Ok(())
    }

    pub(super) fn show_journal_move<D: DataProvider>(&mut self, app: &App<D>) -> Result<()> {
        self.ensure_move_ready(app)?;
        let source = crate::journal_storage::backend(&app.settings)?
            .path()
            .to_path_buf();
        self.popup_stack
            .push(Popup::MoveJournal(Box::new(MoveJournalPopup::new(source))));
        Ok(())
    }

    pub(super) fn handle_journal_move_input<D: DataProvider>(
        &mut self,
        input: &Input,
        app: &App<D>,
    ) -> Result<HandleInputReturnType> {
        let action = match self.popup_stack.last_mut() {
            Some(Popup::MoveJournal(popup)) => popup.handle_input(input),
            _ => return Ok(HandleInputReturnType::Handled),
        };
        let result = match action {
            MoveJournalAction::Keep => Ok(()),
            MoveJournalAction::Close => {
                self.popup_stack.pop();
                Ok(())
            }
            MoveJournalAction::Prepare(destination) => (|| {
                self.ensure_move_ready(app)?;
                let context = app
                    .config_context
                    .as_ref()
                    .context("Configuration paths are unavailable")?;
                let document = self
                    .popup_stack
                    .iter_mut()
                    .find_map(|popup| match popup {
                        Popup::Config(config) => Some(config.settings_document_mut()),
                        _ => None,
                    })
                    .context("Configuration is no longer open")?;
                let request =
                    journal_move::prepare(&app.settings, context, document, &destination)?;
                if let Some(Popup::MoveJournal(popup)) = self.popup_stack.last_mut() {
                    popup.prepared(request);
                }
                Ok(())
            })(),
            MoveJournalAction::Move(request) => match self.ensure_move_ready(app) {
                Ok(()) => {
                    self.requested_journal_move = Some(*request);
                    return Ok(HandleInputReturnType::MoveJournal);
                }
                Err(error) => Err(error),
            },
        };
        if let Err(error) = result {
            if let Some(Popup::MoveJournal(popup)) = self.popup_stack.last_mut() {
                popup.show_error(format!("{error:#}"));
            } else {
                return Err(error);
            }
        }
        Ok(HandleInputReturnType::Handled)
    }

    pub(crate) fn take_journal_move_request(&mut self) -> Option<journal_move::MoveRequest> {
        self.requested_journal_move.take()
    }
}

#[cfg(all(test, feature = "file"))]
mod tests {
    use std::{
        collections::BTreeMap,
        fs,
        path::{Path, PathBuf},
    };

    use backend::{
        EntryDraft, FileFormat,
        journal::{JournalBackend, JournalLease, JournalProvider},
    };
    use crossterm::event::{KeyCode, KeyModifiers};

    use crate::{
        app::ui::{ConfigAction, Styles, entry_popup::EntryPopup},
        settings::{ConfigContext, Settings},
    };

    use super::*;

    async fn fixture() -> (
        tempfile::TempDir,
        App<JournalProvider>,
        UIComponents<'static>,
    ) {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("entries");
        let profile = directory.path().join("profile");
        fs::create_dir(&profile).unwrap();
        let settings: Settings = toml::from_str(&format!(
            "auto_title = 'Daily note'\ncustom_title = 'Remembered title'\nbackend_type = 'File'\napp_state_dir = '{}'\n[file_backend]\ndirectory = '{}'\n",
            profile.join("state").display(), source.display()
        )).unwrap();
        let settings_path = profile.join("config.toml");
        fs::write(&settings_path, toml::to_string(&settings).unwrap()).unwrap();
        let mut provider = JournalProvider::open(JournalBackend::File(source, FileFormat::Plain))
            .await
            .unwrap();
        provider
            .add_entry(
                EntryDraft::new(
                    chrono::Utc::now(),
                    "Existing entry".into(),
                    vec!["tag".into()],
                    None,
                )
                .with_content("Saved content".into()),
            )
            .await
            .unwrap();
        let mut app = App::new(provider, settings);
        app.config_context = Some(ConfigContext {
            journal_name: Some("Personal".into()),
            return_to_journals: true,
            storage_overridden: false,
            settings_path,
            themes_path: profile.join("themes"),
            active_theme_id: Some("tui".into()),
            theme_warning: None,
            launch_details: vec![],
        });
        app.load_entries().await.unwrap();
        let mut ui = UIComponents::new(Styles::default());
        ui.set_current_entry(app.entries.first().map(|entry| entry.id), &mut app);
        ui.show_config(&app).unwrap();
        (directory, app, ui)
    }

    fn key(code: KeyCode) -> Input {
        Input::new(code, KeyModifiers::NONE)
    }

    fn files(directory: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
        fs::read_dir(directory)
            .unwrap()
            .map(|entry| {
                let path = entry.unwrap().path();
                (path.file_name().unwrap().into(), fs::read(&path).unwrap())
            })
            .collect()
    }

    async fn review(ui: &mut UIComponents<'_>, app: &mut App<JournalProvider>, destination: &Path) {
        ui.save_config(ConfigAction::MoveJournal, app).unwrap();
        assert!(matches!(ui.popup_stack.last(), Some(Popup::MoveJournal(_))));
        for ch in destination.to_str().unwrap().chars() {
            assert_eq!(
                ui.handle_input(&key(KeyCode::Char(ch)), app).await.unwrap(),
                HandleInputReturnType::Handled
            );
        }
        assert_eq!(
            ui.handle_input(&key(KeyCode::Enter), app).await.unwrap(),
            HandleInputReturnType::Handled
        );
        assert!(ui.take_journal_move_request().is_none());
    }

    #[tokio::test]
    async fn cancelling_review_keeps_config_and_storage_unchanged() {
        let (directory, mut app, mut ui) = fixture().await;
        let config = app.config_context.as_ref().unwrap().settings_path.clone();
        let original_config = fs::read(&config).unwrap();
        let source = directory.path().join("entries");
        let original_files = files(&source);
        let destination = directory.path().join("new-location");
        review(&mut ui, &mut app, &destination).await;
        ui.handle_input(&key(KeyCode::Esc), &mut app).await.unwrap();
        assert!(matches!(ui.popup_stack.last(), Some(Popup::MoveJournal(_))));
        ui.handle_input(&key(KeyCode::Esc), &mut app).await.unwrap();
        assert!(matches!(ui.popup_stack.last(), Some(Popup::Config(_))));
        assert!(ui.take_journal_move_request().is_none());
        assert!(!destination.exists());
        assert_eq!(fs::read(config).unwrap(), original_config);
        assert_eq!(files(&source), original_files);
    }

    #[tokio::test]
    async fn confirmation_queues_a_move_without_writing_or_requesting_entry_passwords() {
        for whole_journal in [false, true] {
            let (directory, mut app, mut ui) = fixture().await;
            let id = app.current_entry_id.unwrap();
            if whole_journal {
                app.enable_journal_protection("journal password".to_owned().into())
                    .await
                    .unwrap();
            } else {
                app.protect_note(
                    id,
                    "entry password".to_owned().into(),
                    "Saved content".into(),
                )
                .await
                .unwrap();
                assert!(app.is_locked(id));
            }
            ui.set_current_entry(app.current_entry_id, &mut app);
            let config_path = app.config_context.as_ref().unwrap().settings_path.clone();
            let original_config = fs::read(&config_path).unwrap();
            let backend = crate::journal_storage::backend(&app.settings).unwrap();
            let original_files = files(backend.path());
            let original_header = fs::read(backend.header_path()).ok();
            let destination = directory.path().join("new-location");
            review(&mut ui, &mut app, &destination).await;
            assert_eq!(
                ui.handle_input(&key(KeyCode::Enter), &mut app)
                    .await
                    .unwrap(),
                HandleInputReturnType::Handled
            );
            assert_eq!(
                ui.handle_input(&key(KeyCode::Char('y')), &mut app)
                    .await
                    .unwrap(),
                HandleInputReturnType::MoveJournal
            );
            let request = ui.take_journal_move_request().expect("Move was not queued");
            assert_eq!(
                request.destination,
                fs::canonicalize(directory.path())
                    .unwrap()
                    .join("new-location")
            );
            assert!(ui.take_journal_move_request().is_none());
            assert!(!destination.exists());
            assert_eq!(fs::read(config_path).unwrap(), original_config);
            assert_eq!(files(backend.path()), original_files);
            assert_eq!(fs::read(backend.header_path()).ok(), original_header);
            assert!(JournalLease::acquire(&backend).is_err());
            assert!(app.data_provide.load_all_entries().await.is_ok());
        }
    }

    #[tokio::test]
    async fn move_rejects_unsaved_entry_metadata_config_overrides_and_reload_requirements() {
        let (_directory, mut app, mut ui) = fixture().await;
        ui.editor.set_entry_content("Unsaved content", &app);
        assert!(
            ui.save_config(ConfigAction::MoveJournal, &mut app)
                .unwrap_err()
                .to_string()
                .contains("entry edits")
        );
        ui.set_current_entry(app.current_entry_id, &mut app);
        ui.popup_stack
            .push(Popup::Entry(Box::new(EntryPopup::new_entry(&app.settings))));
        assert!(
            ui.save_config(ConfigAction::MoveJournal, &mut app)
                .unwrap_err()
                .to_string()
                .contains("entry edits")
        );
        ui.popup_stack.pop();

        let Popup::Config(config) = ui.popup_stack.last_mut().unwrap() else {
            panic!("Missing config")
        };
        let original_document = config.settings_document_mut().text();
        config
            .settings_document_mut()
            .set(&["auto_title"], Some("Unsaved title".into()))
            .unwrap();
        assert!(
            ui.save_config(ConfigAction::MoveJournal, &mut app)
                .unwrap_err()
                .to_string()
                .contains("configuration changes")
        );
        let Popup::Config(config) = ui.popup_stack.last_mut().unwrap() else {
            panic!("Missing config")
        };
        config
            .settings_document_mut()
            .replace_text(&original_document)
            .unwrap();

        app.config_context.as_mut().unwrap().storage_overridden = true;
        assert!(
            ui.save_config(ConfigAction::MoveJournal, &mut app)
                .unwrap_err()
                .to_string()
                .contains("storage overrides")
        );
        app.config_context.as_mut().unwrap().storage_overridden = false;
        app.data_provide.journal_records().await.unwrap();
        assert!(app.data_provide.journal_reload_required());
        assert!(
            ui.save_config(ConfigAction::MoveJournal, &mut app)
                .unwrap_err()
                .to_string()
                .contains("reopen")
        );
        assert!(matches!(ui.popup_stack.last(), Some(Popup::Config(_))));
        assert!(ui.take_journal_move_request().is_none());
    }

    #[tokio::test]
    async fn move_rejects_sealed_drafts_even_when_no_editor_change_is_visible() {
        let (_directory, mut app, mut ui) = fixture().await;
        let id = app.current_entry_id.unwrap();
        app.protect_note(
            id,
            "entry password".to_owned().into(),
            "Saved content".into(),
        )
        .await
        .unwrap();
        app.unlock_note(id, "entry password".to_owned().into())
            .await
            .unwrap();
        app.seal_draft_and_lock(id, "Unfinished private draft".into(), None)
            .await
            .unwrap();
        ui.set_current_entry(Some(id), &mut app);
        assert!(!ui.has_unsaved());
        assert!(app.has_sealed_drafts());
        assert!(
            ui.save_config(ConfigAction::MoveJournal, &mut app)
                .unwrap_err()
                .to_string()
                .contains("entry edits")
        );
        assert!(app.has_sealed_drafts());
        assert!(ui.take_journal_move_request().is_none());
    }

    #[tokio::test]
    async fn confirmation_rechecks_session_guards_before_queueing() {
        let (directory, mut app, mut ui) = fixture().await;
        let destination = directory.path().join("new-location");
        review(&mut ui, &mut app, &destination).await;
        app.config_context.as_mut().unwrap().storage_overridden = true;
        assert_eq!(
            ui.handle_input(&key(KeyCode::Char('y')), &mut app)
                .await
                .unwrap(),
            HandleInputReturnType::Handled
        );
        assert!(matches!(ui.popup_stack.last(), Some(Popup::MoveJournal(_))));
        assert!(ui.take_journal_move_request().is_none());
        assert!(!destination.exists());
    }
}

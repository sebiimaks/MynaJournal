use anyhow::{Context, Result};
use crossterm::event::{Event, EventStream, KeyEventKind};
use ratatui::{Terminal, backend::Backend};

use crate::app::{App, UIComponents};
use crate::cli::PendingCliCommand;
use crate::settings::{ConfigContext, Settings};
use futures_util::StreamExt;

use backend::{DataProvider, journal::JournalProvider};

use super::keymap::Input;
use super::ui::Styles;
use super::ui::ui_functions::render_message_centered;

#[derive(Debug, PartialEq, Eq)]
pub enum HandleInputReturnType {
    Handled,
    NotFound,
    ExitApp,
    MoveJournal,
    Ignore,
}

#[derive(Debug, PartialEq, Eq)]
pub enum RunOutcome {
    JournalClosed,
    InputEnded,
}

#[derive(Debug)]
enum SessionOutcome {
    Finished(RunOutcome),
    Move(Box<crate::journal_move::MoveRequest>),
}

pub async fn run<B: Backend>(
    terminal: &mut Terminal<B>,
    settings: Settings,
    styles: Styles,
    pending_cmd: Option<PendingCliCommand>,
    config_context: ConfigContext,
) -> Result<RunOutcome>
where
    B::Error: Send + Sync + 'static,
{
    let backend = crate::journal_storage::backend(&settings)?;
    let mut data_provider = JournalProvider::open(backend).await?;
    if data_provider.is_journal_protected() {
        match unlock_journal(terminal, &mut data_provider, &styles).await? {
            UnlockOutcome::Unlocked => {}
            UnlockOutcome::Cancelled => return Ok(RunOutcome::JournalClosed),
            UnlockOutcome::InputEnded => return Ok(RunOutcome::InputEnded),
        }
    }
    let message_styles = styles.clone();
    let outcome = run_intern(
        terminal,
        data_provider,
        settings,
        styles,
        pending_cmd,
        config_context,
    )
    .await?;
    match outcome {
        SessionOutcome::Finished(outcome) => Ok(outcome),
        SessionOutcome::Move(request) => {
            terminal.draw(|frame| render_message_centered(frame, "Moving journal..."))?;
            let result = crate::journal_move::execute(*request).await;
            let (message, error) = match result {
                Ok(report) => {
                    let mut text = format!(
                        "Journal moved.\n\nStorage: {}\n\nRecovery copy: {}\n\nReopen the journal to continue.",
                        report.destination.display(),
                        report.recovery_path.display(),
                    );
                    if let Some(warning) = report.warning {
                        text.push_str(&format!("\n\n{warning}"));
                    }
                    (text, false)
                }
                Err(error) => (
                    format!(
                        "Move could not be completed.\n\n{error:#}\n\nReopen the journal to continue."
                    ),
                    true,
                ),
            };
            show_move_result(terminal, &message_styles, message, error).await
        }
    }
}

async fn show_move_result<B: Backend>(
    terminal: &mut Terminal<B>,
    styles: &Styles,
    message: String,
    error: bool,
) -> Result<RunOutcome>
where
    B::Error: Send + Sync + 'static,
{
    use super::ui::journal_move_popup::{MoveJournalAction, MoveJournalPopup};
    crate::terminal_session::clear_fullscreen(terminal)?;
    let mut popup = MoveJournalPopup::result(message, error);
    terminal.draw(|frame| popup.render_widget(frame, frame.area(), styles))?;
    let mut events = EventStream::new();
    loop {
        let Some(event) = events.next().await else {
            return Ok(RunOutcome::InputEnded);
        };
        if let Event::Key(key) = event.context("Reading move result input")?
            && key.kind == KeyEventKind::Press
            && matches!(
                popup.handle_input(&Input::from(&key)),
                MoveJournalAction::Close
            )
        {
            return Ok(RunOutcome::JournalClosed);
        }
        terminal.draw(|frame| popup.render_widget(frame, frame.area(), styles))?;
    }
}

enum UnlockOutcome {
    Unlocked,
    Cancelled,
    InputEnded,
}

async fn unlock_journal<B: Backend>(
    terminal: &mut Terminal<B>,
    provider: &mut JournalProvider,
    styles: &Styles,
) -> Result<UnlockOutcome>
where
    B::Error: Send + Sync + 'static,
{
    use super::ui::journal_security_popup::{
        JournalPasswordAction, JournalSecurityAction, JournalSecurityPopup,
    };
    crate::terminal_session::clear_fullscreen(terminal)?;
    let mut popup = JournalSecurityPopup::new(JournalPasswordAction::Unlock);
    // Complete the first terminal redraw before starting the shared input reader,
    // just as the main journal runner does.
    terminal.draw(|frame| popup.render_widget(frame, frame.area(), styles))?;
    let mut events = EventStream::new();
    loop {
        let Some(event) = events.next().await else {
            return Ok(UnlockOutcome::InputEnded);
        };
        if let Event::Key(key) = event.context("Reading journal password input")?
            && key.kind == KeyEventKind::Press
        {
            match popup.handle_input(&Input::from(&key)) {
                JournalSecurityAction::Close => return Ok(UnlockOutcome::Cancelled),
                JournalSecurityAction::Unlock(password) => {
                    terminal
                        .draw(|frame| render_message_centered(frame, "Unlocking journal..."))?;
                    match provider.unlock(password).await {
                        Ok(()) => return Ok(UnlockOutcome::Unlocked),
                        Err(error) => popup.show_error(error.to_string()),
                    }
                }
                _ => {}
            }
        }
        terminal.draw(|frame| popup.render_widget(frame, frame.area(), styles))?;
    }
}

async fn run_intern<B, D>(
    terminal: &mut Terminal<B>,
    data_provider: D,
    settings: Settings,
    styles: Styles,
    pending_cmd: Option<PendingCliCommand>,
    config_context: ConfigContext,
) -> anyhow::Result<SessionOutcome>
where
    B: Backend,
    B::Error: Send + Sync + 'static,
    D: DataProvider,
{
    // Some terminals keep the old display when entering the alternate screen.
    crate::terminal_session::clear_fullscreen(terminal)?;
    let mut app = App::new(data_provider, settings);
    app.set_tag_palette(&styles.tags.palette);
    let mut ui_components = UIComponents::new(styles);
    ui_components.set_active_theme(config_context.active_theme_id.clone());
    if let Some(warning) = &config_context.theme_warning {
        ui_components.show_err_msg(warning.clone());
    }
    let show_startup_artwork = config_context.journal_name.is_none();
    app.config_context = Some(config_context);
    if let Some(cmd) = pending_cmd
        && let Err(err) = exec_pending_cmd(terminal, &mut app, cmd).await
    {
        ui_components.show_err_msg(err.to_string());
    }

    app.load_state(&mut ui_components);

    if let Err(err) = app.load_entries().await {
        ui_components.show_err_msg(err.to_string());
    }

    ui_components.set_current_entry(app.entries.first().map(|entry| entry.id), &mut app);
    if show_startup_artwork {
        ui_components.show_startup_artwork();
    }

    draw_ui(terminal, &mut app, &mut ui_components)?;

    let mut input_stream = EventStream::new();
    let mut security_tick = tokio::time::interval(std::time::Duration::from_secs(1));
    loop {
        let event = tokio::select! {
            biased;
            _ = security_tick.tick() => {
                if app.security_expired() && !ui_components.is_privacy_shielded() && !ui_components.journal_conversion_pending() {
                    auto_lock(terminal, &mut app, &mut ui_components).await?;
                }
                continue;
            }
            event = input_stream.next() => event,
        };
        let Some(event) = event else {
            break;
        };
        let event = event.context("Error getting input stream")?;
        if app.security_expired()
            && !ui_components.is_privacy_shielded()
            && !ui_components.journal_conversion_pending()
        {
            auto_lock(terminal, &mut app, &mut ui_components).await?;
            continue;
        }
        if matches!(&event, Event::Key(key) if key.kind == KeyEventKind::Press)
            || matches!(event, Event::Mouse(mouse) if !matches!(mouse.kind, crossterm::event::MouseEventKind::Moved))
        {
            app.record_activity();
        }
        match handle_input(event, &mut app, &mut ui_components).await {
            Ok(result) => {
                if ui_components.take_lock_all_request() {
                    auto_lock(terminal, &mut app, &mut ui_components).await?;
                }
                match result {
                    HandleInputReturnType::Handled => {
                        ui_components.update_current_entry(&mut app);
                        draw_ui(terminal, &mut app, &mut ui_components)?;
                    }
                    HandleInputReturnType::NotFound => {
                        // UI should be drawn even if the input isn't handled in the app logic to
                        // catch events like resize, Font resize, Mouse activation...
                        draw_ui(terminal, &mut app, &mut ui_components)?;
                    }
                    HandleInputReturnType::ExitApp => {
                        // Logging persisting errors by closing the app is enough
                        if let Err(err) = app.persist_state() {
                            log::error!("Persisting app state failed: Error info {err}");
                        }

                        return Ok(SessionOutcome::Finished(RunOutcome::JournalClosed));
                    }
                    HandleInputReturnType::MoveJournal => {
                        let request = ui_components
                            .take_journal_move_request()
                            .context("Move request is missing")?;
                        app.persist_state()
                            .context("Saving journal state before moving")?;
                        app.data_provide.close_for_move().await?;
                        // Returning drops the app, its secrets and storage leases before the move.
                        return Ok(SessionOutcome::Move(Box::new(request)));
                    }
                    HandleInputReturnType::Ignore => {}
                };
            }
            Err(err) => {
                ui_components.show_err_msg(err.to_string());
                draw_ui(terminal, &mut app, &mut ui_components)?;
            }
        }
    }

    Ok(SessionOutcome::Finished(RunOutcome::InputEnded))
}

async fn exec_pending_cmd<B: Backend, D: DataProvider>(
    terminal: &mut Terminal<B>,
    app: &mut App<D>,
    pending_cmd: PendingCliCommand,
) -> anyhow::Result<()>
where
    B::Error: Send + Sync + 'static,
{
    match pending_cmd {
        PendingCliCommand::ImportJournals(file_path) => {
            terminal.draw(|f| render_message_centered(f, "Importing journals..."))?;

            app.import_entries(file_path).await?;
        }
        PendingCliCommand::AssignPriority(priority) => {
            terminal.draw(|f| render_message_centered(f, "Assigning Priority to Journals..."))?;
            app.assign_priority_to_entries(priority).await?;
        }
    }

    Ok(())
}

async fn auto_lock<B: Backend, D: DataProvider>(
    terminal: &mut Terminal<B>,
    app: &mut App<D>,
    ui: &mut UIComponents<'_>,
) -> anyhow::Result<()>
where
    B::Error: Send + Sync + 'static,
{
    ui.hide_for_auto_lock();
    crate::terminal_session::clear_fullscreen(terminal)?;
    draw_ui(terminal, app, ui)?;
    if let Err(error) = ui.auto_lock(app).await {
        ui.recover_failed_auto_lock(app, &error)?;
    }
    draw_ui(terminal, app, ui)
}

fn draw_ui<B: Backend, D: DataProvider>(
    terminal: &mut Terminal<B>,
    app: &mut App<D>,
    ui_components: &mut UIComponents,
) -> anyhow::Result<()>
where
    B::Error: Send + Sync + 'static,
{
    if app.redraw_after_restore {
        app.redraw_after_restore = false;
        // Apply hide cursor again after closing the external editor
        terminal.hide_cursor()?;
        // Clear the terminal and force a full redraw on the next draw call.
        crate::terminal_session::clear_fullscreen(terminal)?;
    }

    terminal.draw(|f| ui_components.render_ui(f, app))?;

    Ok(())
}

async fn handle_input<D: DataProvider>(
    event: Event,
    app: &mut App<D>,
    ui_components: &mut UIComponents<'_>,
) -> Result<HandleInputReturnType> {
    if ui_components.is_privacy_shielded() && !matches!(event, Event::Key(_) | Event::Resize(_, _))
    {
        return Ok(HandleInputReturnType::Ignore);
    }
    let concealed_word = (matches!(event, Event::FocusLost | Event::Paste(_))
        || matches!(&event, Event::Mouse(mouse) if !matches!(mouse.kind, crossterm::event::MouseEventKind::Moved)))
        && ui_components.conceal_active_word();
    let hidden_keypress = ui_components.is_details_hidden()
        && matches!(&event, Event::Key(key) if key.kind == KeyEventKind::Press);
    let user_action = matches!(&event, Event::Key(key) if key.kind == KeyEventKind::Press)
        || matches!(&event, Event::Mouse(mouse) if !matches!(mouse.kind, crossterm::event::MouseEventKind::Moved))
        || matches!(&event, Event::Paste(_));
    let dismissed_artwork = user_action && ui_components.dismiss_startup_artwork();
    let result = match event {
        Event::Key(key) => match key.kind {
            KeyEventKind::Press => {
                let input = Input::from(&key);
                ui_components.handle_input(&input, app).await
            }
            KeyEventKind::Repeat | KeyEventKind::Release => Ok(HandleInputReturnType::Ignore),
        },
        Event::Mouse(mouse) => Ok(ui_components.handle_mouse_input(mouse, app)),
        Event::Resize(_, _) => {
            ui_components.cancel_divider_drag();
            Ok(HandleInputReturnType::NotFound)
        }
        _ => Ok(HandleInputReturnType::NotFound),
    };
    if (dismissed_artwork || concealed_word || hidden_keypress)
        && matches!(result, Ok(HandleInputReturnType::Ignore))
    {
        Ok(HandleInputReturnType::NotFound)
    } else {
        result
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use ratatui::{
        backend::{TestBackend, WindowSize},
        buffer::{Buffer, Cell},
        layout::{Position, Size},
    };

    use super::*;

    #[tokio::test]
    async fn hidden_active_word_is_repainted_after_focus_loss_paste_and_ignored_mouse_actions() {
        use crate::app::ui::{ControlType, UICommand};
        use crossterm::event::{KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind};
        let mut app = crate::app::test::create_default_app();
        app.data_provide.journal_protected = true;
        app.load_entries().await.unwrap();
        let mut ui = UIComponents::new(Styles::default());
        ui.set_current_entry(Some(0), &mut app);
        ui.change_active_control(ControlType::EntryContentTxt);
        ui.handle_input(
            &Input::new(KeyCode::Char('o'), KeyModifiers::CONTROL),
            &mut app,
        )
        .await
        .unwrap();
        let mut terminal = Terminal::new(TestBackend::new(100, 20)).unwrap();
        for event in [
            Event::FocusLost,
            Event::Paste("pastedprivateword".into()),
            Event::Mouse(MouseEvent {
                kind: MouseEventKind::Down(MouseButton::Left),
                column: 80,
                row: 3,
                modifiers: KeyModifiers::NONE,
            }),
        ] {
            app.entries
                .iter_mut()
                .find(|e| e.id == 0)
                .unwrap()
                .content
                .clear();
            ui.set_current_entry(Some(0), &mut app);
            UICommand::StartEditEntryContent
                .execute(&mut ui, &mut app)
                .await
                .unwrap();
            for c in "visibleword".chars() {
                ui.handle_input(&Input::new(KeyCode::Char(c), KeyModifiers::NONE), &mut app)
                    .await
                    .unwrap();
            }
            terminal.draw(|frame| ui.render_ui(frame, &app)).unwrap();
            let before: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(Cell::symbol)
                .collect();
            assert!(before.contains("visibleword"));
            assert_ne!(
                handle_input(event, &mut app, &mut ui).await.unwrap(),
                HandleInputReturnType::Ignore
            );
            terminal.draw(|frame| ui.render_ui(frame, &app)).unwrap();
            let after: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(Cell::symbol)
                .collect();
            assert!(!after.contains("visibleword"));
            assert!(!after.contains("pastedprivateword"));
            UICommand::SaveEntryContent
                .execute(&mut ui, &mut app)
                .await
                .unwrap();
            assert_eq!(app.get_current_entry().unwrap().content, "visibleword");
        }
    }

    struct StartupBackend {
        screen: TestBackend,
        frames: Vec<Buffer>,
        fail_clear: bool,
        fail_cursor_query: bool,
    }

    impl StartupBackend {
        fn dirty(width: u16, height: u16) -> Self {
            Self {
                screen: TestBackend::with_lines(vec!["~".repeat(width.into()); height.into()]),
                frames: Vec::new(),
                fail_clear: false,
                fail_cursor_query: false,
            }
        }
    }

    impl Backend for StartupBackend {
        type Error = io::Error;

        fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
        where
            I: Iterator<Item = (u16, u16, &'a Cell)>,
        {
            self.screen.draw(content).unwrap();
            self.frames.push(self.screen.buffer().clone());
            // Stop before startup reaches the real terminal's event stream.
            Err(io::Error::other("stopped after frame"))
        }

        fn hide_cursor(&mut self) -> io::Result<()> {
            self.screen.hide_cursor().unwrap();
            Ok(())
        }

        fn show_cursor(&mut self) -> io::Result<()> {
            self.screen.show_cursor().unwrap();
            Ok(())
        }

        fn get_cursor_position(&mut self) -> io::Result<Position> {
            if self.fail_cursor_query {
                return Err(io::Error::other(
                    "cursor queries conflict with the event reader",
                ));
            }
            Ok(self.screen.get_cursor_position().unwrap())
        }

        fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
            self.screen.set_cursor_position(position).unwrap();
            Ok(())
        }

        fn clear(&mut self) -> io::Result<()> {
            if self.fail_clear {
                return Err(io::Error::other("clearing screen failed"));
            }
            self.screen.clear().unwrap();
            Ok(())
        }

        fn clear_region(&mut self, clear_type: ratatui::backend::ClearType) -> io::Result<()> {
            if self.fail_clear {
                return Err(io::Error::other("clearing screen failed"));
            }
            self.screen.clear_region(clear_type).unwrap();
            Ok(())
        }

        fn size(&self) -> io::Result<Size> {
            Ok(self.screen.size().unwrap())
        }

        fn window_size(&mut self) -> io::Result<WindowSize> {
            Ok(self.screen.window_size().unwrap())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.screen.flush().unwrap();
            Ok(())
        }
    }

    #[tokio::test]
    async fn lock_all_hides_private_content_before_sealing_drafts() {
        use crate::app::ui::UICommand;
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let mut app = crate::app::test::create_default_app();
        app.load_entries().await.unwrap();
        app.protect_note(
            0,
            "test passphrase".to_owned().into(),
            "Secret note body".into(),
        )
        .await
        .unwrap();
        app.unlock_note(0, "test passphrase".to_owned().into())
            .await
            .unwrap();
        let mut ui = UIComponents::new(Styles::default());
        ui.set_current_entry(Some(0), &mut app);
        UICommand::StartEditEntryContent
            .execute(&mut ui, &mut app)
            .await
            .unwrap();
        handle_input(
            Event::Key(KeyEvent::new(KeyCode::Char('X'), KeyModifiers::NONE)),
            &mut app,
            &mut ui,
        )
        .await
        .unwrap();
        assert!(ui.has_unsaved());
        handle_input(
            Event::Key(KeyEvent::new(KeyCode::Char('l'), KeyModifiers::CONTROL)),
            &mut app,
            &mut ui,
        )
        .await
        .unwrap();
        assert!(ui.take_lock_all_request());
        let mut terminal = Terminal::new(StartupBackend::dirty(100, 30)).unwrap();
        assert!(auto_lock(&mut terminal, &mut app, &mut ui).await.is_err());
        assert!(ui.is_privacy_shielded());
        assert!(!app.is_locked(0));
        assert!(ui.has_unsaved());
        assert!(!app.has_sealed_drafts());
        let frame = terminal.backend().frames.last().unwrap();
        let text: String = frame.content.iter().map(|cell| cell.symbol()).collect();
        assert!(!text.contains("Secret note body"));
        assert!(!text.contains('~'));
    }

    async fn startup(
        backend: StartupBackend,
        pending_cmd: Option<PendingCliCommand>,
    ) -> (Terminal<StartupBackend>, anyhow::Error) {
        let dir = tempfile::tempdir().unwrap();
        let mut app = crate::app::test::create_default_app();
        let mut settings = app.settings.clone();
        settings.app_state_dir = Some(dir.path().join("state"));
        settings.sync_os_clipboard = false;
        let context = ConfigContext {
            journal_name: None,
            return_to_journals: false,
            storage_overridden: false,
            settings_path: dir.path().join("config.toml"),
            themes_path: dir.path().join("themes"),
            active_theme_id: Some("tui".into()),
            theme_warning: None,
            launch_details: Vec::new(),
        };
        let mut terminal = Terminal::new(backend).unwrap();
        let error = run_intern(
            &mut terminal,
            std::mem::take(&mut app.data_provide),
            settings,
            Styles::default(),
            pending_cmd,
            context,
        )
        .await
        .unwrap_err();
        (terminal, error)
    }

    fn assert_clean_first_frame(terminal: &Terminal<StartupBackend>, expected: &str) {
        let frame = &terminal.backend().frames[0];
        let text: String = frame.content.iter().map(Cell::symbol).collect();
        assert!(
            !text.contains('~'),
            "Previous terminal contents survived startup"
        );
        assert!(
            text.contains(expected),
            "Missing first frame text: {expected}"
        );
    }

    #[cfg(feature = "json")]
    #[tokio::test]
    async fn protected_startup_draws_only_password_prompt_before_reading_entries() {
        let temporary = tempfile::tempdir().unwrap();
        let storage = backend::journal::JournalBackend::Json(temporary.path().join("entries.json"));
        let mut provider = JournalProvider::open(storage).await.unwrap();
        provider
            .add_entry(
                backend::EntryDraft::new(
                    chrono::Utc::now(),
                    "PRIVATE_STARTUP_TITLE".into(),
                    vec![],
                    None,
                )
                .with_content("PRIVATE_STARTUP_BODY".into()),
            )
            .await
            .unwrap();
        provider
            .enable_journal_protection("startup password".to_owned().into(), None)
            .await
            .unwrap();
        drop(provider);
        let mut settings = Settings::default();
        settings.backend_type = Some(crate::settings::BackendType::Json);
        settings.json_backend.file_path = Some(temporary.path().join("entries.json"));
        settings.app_state_dir = Some(temporary.path().join("state"));
        let context = ConfigContext {
            journal_name: Some("Personal".into()),
            return_to_journals: true,
            storage_overridden: false,
            settings_path: temporary.path().join("config.toml"),
            themes_path: temporary.path().join("themes"),
            active_theme_id: None,
            theme_warning: None,
            launch_details: vec![],
        };
        let mut terminal = Terminal::new(StartupBackend::dirty(100, 30)).unwrap();
        assert!(
            run(&mut terminal, settings, Styles::default(), None, context)
                .await
                .is_err()
        );
        assert_clean_first_frame(&terminal, "Unlock Journal");
        let frame = &terminal.backend().frames[0];
        let text: String = frame.content.iter().map(Cell::symbol).collect();
        assert!(!text.contains("PRIVATE_STARTUP"));
    }

    #[tokio::test]
    async fn opening_a_journal_redraws_without_querying_the_cursor() {
        let mut backend = StartupBackend::dirty(80, 24);
        backend.fail_cursor_query = true;
        let (terminal, error) = startup(backend, None).await;
        assert!(error.to_string().contains("stopped after frame"));
        assert_clean_first_frame(&terminal, "Entries");
    }

    #[tokio::test]
    async fn initial_journal_frame_clears_previous_terminal_contents() {
        let (terminal, error) = startup(StartupBackend::dirty(80, 24), None).await;
        assert_eq!(error.to_string(), "stopped after frame");
        assert_clean_first_frame(&terminal, "Title 2");
    }

    #[tokio::test]
    async fn small_terminal_startup_clears_previous_contents() {
        let (terminal, error) = startup(StartupBackend::dirty(19, 3), None).await;
        assert_eq!(error.to_string(), "stopped after frame");
        assert_clean_first_frame(&terminal, "Terminal too small");
    }

    #[tokio::test]
    async fn import_progress_clears_previous_terminal_contents() {
        let (terminal, error) = startup(
            StartupBackend::dirty(80, 24),
            Some(PendingCliCommand::ImportJournals("unused.json".into())),
        )
        .await;
        assert_eq!(error.to_string(), "stopped after frame");
        assert_clean_first_frame(&terminal, "Importing journals...");
    }

    #[tokio::test]
    async fn priority_progress_clears_previous_terminal_contents() {
        let (terminal, error) = startup(
            StartupBackend::dirty(80, 24),
            Some(PendingCliCommand::AssignPriority(1)),
        )
        .await;
        assert_eq!(error.to_string(), "stopped after frame");
        assert_clean_first_frame(&terminal, "Assigning Priority to Journals...");
    }

    #[tokio::test]
    async fn startup_clear_failure_stops_before_drawing() {
        let mut backend = StartupBackend::dirty(80, 24);
        backend.fail_clear = true;
        let (terminal, error) = startup(backend, Some(PendingCliCommand::AssignPriority(1))).await;
        assert_eq!(error.to_string(), "clearing screen failed");
        assert!(terminal.backend().frames.is_empty());
    }
}

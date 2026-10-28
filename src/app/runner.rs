use anyhow::{Context, Result};
use crossterm::event::{Event, EventStream, KeyEventKind};
use ratatui::{Terminal, backend::Backend};

use crate::app::{App, UIComponents};
use crate::cli::PendingCliCommand;
use crate::settings::{BackendType, ConfigContext, Settings};
use futures_util::StreamExt;

use backend::DataProvider;
#[cfg(feature = "file")]
use backend::FileDataProvider;
#[cfg(feature = "json")]
use backend::JsonDataProvide;
#[cfg(feature = "sqlite")]
use backend::SqliteDataProvide;
#[cfg(feature = "vjournal")]
use backend::VjournalDataProvide;

use super::keymap::Input;
use super::ui::Styles;
use super::ui::ui_functions::render_message_centered;

#[derive(Debug, PartialEq, Eq)]
pub enum HandleInputReturnType {
    Handled,
    NotFound,
    ExitApp,
    Ignore,
}

pub async fn run<B: Backend>(
    terminal: &mut Terminal<B>,
    settings: Settings,
    styles: Styles,
    pending_cmd: Option<PendingCliCommand>,
    config_context: ConfigContext,
) -> Result<()>
where
    B::Error: Send + Sync + 'static,
{
    match settings.backend_type.unwrap_or_default() {
        #[cfg(feature = "json")]
        BackendType::Json => {
            let path = if let Some(path) = &settings.json_backend.file_path {
                path.clone()
            } else {
                crate::settings::json_backend::get_default_json_path()?
            };
            let data_provider = JsonDataProvide::new(path);
            run_intern(
                terminal,
                data_provider,
                settings,
                styles,
                pending_cmd,
                config_context,
            )
            .await
        }
        #[cfg(not(feature = "json"))]
        BackendType::Json => {
            anyhow::bail!(
                "Feature 'json' is not installed. Please check your configs and set your backend to an installed feature, or reinstall the program with 'json' feature"
            )
        }
        #[cfg(feature = "sqlite")]
        BackendType::Sqlite => {
            let path = if let Some(path) = &settings.sqlite_backend.file_path {
                path.clone()
            } else {
                crate::settings::sqlite_backend::get_default_sqlite_path()?
            };
            let data_provider = SqliteDataProvide::from_file(path).await?;
            run_intern(
                terminal,
                data_provider,
                settings,
                styles,
                pending_cmd,
                config_context,
            )
            .await
        }
        #[cfg(not(feature = "sqlite"))]
        BackendType::Sqlite => {
            anyhow::bail!(
                "Feature 'sqlite' is not installed. Please check your configs and set your backend to an installed feature, or reinstall the program with 'sqlite' feature"
            )
        }
        #[cfg(feature = "vjournal")]
        BackendType::Vjournal => {
            let directory = if let Some(dir) = &settings.vjournal_backend.directory {
                dir.clone()
            } else {
                crate::settings::vjournal_backend::get_default_vjournal_path()?
            };
            let data_provider = VjournalDataProvide::new(directory);
            run_intern(
                terminal,
                data_provider,
                settings,
                styles,
                pending_cmd,
                config_context,
            )
            .await
        }
        #[cfg(not(feature = "vjournal"))]
        BackendType::Vjournal => {
            anyhow::bail!(
                "Feature 'vjournal' is not installed. Please check your configs and set your backend to an installed feature, or reinstall the program with 'vjournal' feature"
            )
        }
        #[cfg(feature = "file")]
        BackendType::File => {
            let directory = settings
                .file_backend
                .directory
                .clone()
                .map_or_else(crate::settings::file_backend::get_default_file_path, Ok)?;
            let data_provider = FileDataProvider::new(directory, settings.file_backend.format);
            run_intern(
                terminal,
                data_provider,
                settings,
                styles,
                pending_cmd,
                config_context,
            )
            .await
        }
        #[cfg(not(feature = "file"))]
        BackendType::File => {
            anyhow::bail!(
                "Feature 'file' is not installed. Please check your configs and set your backend to an installed feature, or reinstall the program with 'file' feature"
            )
        }
    }
}

async fn run_intern<B, D>(
    terminal: &mut Terminal<B>,
    data_provider: D,
    settings: Settings,
    styles: Styles,
    pending_cmd: Option<PendingCliCommand>,
    config_context: ConfigContext,
) -> anyhow::Result<()>
where
    B: Backend,
    B::Error: Send + Sync + 'static,
    D: DataProvider,
{
    // Some terminals keep the old display when entering the alternate screen.
    terminal.clear()?;
    let mut app = App::new(data_provider, settings);
    app.set_tag_palette(&styles.tags.palette);
    let mut ui_components = UIComponents::new(styles);
    ui_components.set_active_theme(config_context.active_theme_id.clone());
    if let Some(warning) = &config_context.theme_warning {
        ui_components.show_err_msg(warning.clone());
    }
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
    ui_components.show_startup_artwork();

    draw_ui(terminal, &mut app, &mut ui_components)?;

    let mut input_stream = EventStream::new();
    let mut security_tick = tokio::time::interval(std::time::Duration::from_secs(1));
    loop {
        let event = tokio::select! {
            biased;
            _ = security_tick.tick() => {
                if app.security_expired() && !ui_components.is_privacy_shielded() {
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
        if app.security_expired() && !ui_components.is_privacy_shielded() {
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

                        return Ok(());
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

    Ok(())
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
    terminal.clear()?;
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
        terminal.clear()?;
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
    if dismissed_artwork && matches!(result, Ok(HandleInputReturnType::Ignore)) {
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

    struct StartupBackend {
        screen: TestBackend,
        frames: Vec<Buffer>,
        fail_clear: bool,
    }

    impl StartupBackend {
        fn dirty(width: u16, height: u16) -> Self {
            Self {
                screen: TestBackend::with_lines(vec!["~".repeat(width.into()); height.into()]),
                frames: Vec::new(),
                fail_clear: false,
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
        let app = crate::app::test::create_default_app();
        let mut settings = app.settings;
        settings.app_state_dir = Some(dir.path().join("state"));
        settings.sync_os_clipboard = false;
        let context = ConfigContext {
            settings_path: dir.path().join("config.toml"),
            themes_path: dir.path().join("themes"),
            active_theme_id: Some("tui".into()),
            theme_warning: None,
            launch_details: Vec::new(),
        };
        let mut terminal = Terminal::new(backend).unwrap();
        let error = run_intern(
            &mut terminal,
            app.data_provide,
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

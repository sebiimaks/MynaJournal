use std::io;

use anyhow::Result;
use app::ui::themes::ThemeStore;
use clap::Parser;
use ratatui::{Terminal, backend::CrosstermBackend};
use settings::Settings;

mod app;
mod cli;
mod logging;
mod settings;
mod terminal_session;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = cli::Cli::parse();

    let custom_config = cli.config_path.clone();
    let mut settings = Settings::new(custom_config.clone()).await?;
    let launch_details = cli.launch_details();

    let mut pending_cmd = None;

    match cli.handle_cli(&mut settings).await? {
        cli::CliResult::Return => return Ok(()),
        cli::CliResult::Continue => {}
        cli::CliResult::PendingCommand(cmd) => pending_cmd = Some(cmd),
    }

    let theme_store = ThemeStore::new(custom_config.as_ref())?;
    let initialization_warning = theme_store
        .initialize()
        .err()
        .map(|error| format!("Could not install all default theme files: {error:#}"));
    let mut loaded_theme = theme_store.resolve(settings.theme.as_deref());
    if let Some(warning) = initialization_warning {
        loaded_theme.warning = Some(match loaded_theme.warning {
            Some(theme_warning) => format!("{warning}\n{theme_warning}"),
            None => warning,
        });
    }
    let styles = loaded_theme.styles;
    let config_context = settings::ConfigContext {
        settings_path: Settings::file_path(custom_config.clone())?,
        themes_path: theme_store.directory().to_path_buf(),
        active_theme_id: loaded_theme.id,
        theme_warning: loaded_theme.warning,
        launch_details: launch_details?,
    };
    chain_panic_hook();
    let _terminal_session = terminal_session::TerminalSession::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    app::run(&mut terminal, settings, styles, pending_cmd, config_context)
        .await
        .inspect_err(|err| {
            log::error!("[PANIC] {err:?}");
        })?;

    Ok(())
}

/// Clean up the terminal properly if the program panics
fn chain_panic_hook() {
    let original_hook = std::panic::take_hook();

    std::panic::set_hook(Box::new(move |panic| {
        terminal_session::restore_terminal();
        original_hook(panic);
    }));
}

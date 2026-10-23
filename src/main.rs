use std::io;

use anyhow::{Context, Result};
use app::ui::Styles;
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

    let styles =
        Styles::load(custom_config.as_ref()).context("Error while retrieving app styles")?;
    let config_context = settings::ConfigContext {
        settings_path: Settings::file_path(custom_config.clone())?,
        themes_path: std::path::absolute(Styles::file_path(custom_config.as_ref())?)?,
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

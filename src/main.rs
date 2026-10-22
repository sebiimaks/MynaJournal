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

    let mut pending_cmd = None;

    match cli.handle_cli(&mut settings).await? {
        cli::CliResult::Return => return Ok(()),
        cli::CliResult::Continue => {}
        cli::CliResult::PendingCommand(cmd) => pending_cmd = Some(cmd),
    }

    let styles =
        Styles::load(custom_config.as_ref()).context("Error while retrieving app styles")?;
    chain_panic_hook();
    let _terminal_session = terminal_session::TerminalSession::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;

    app::run(&mut terminal, settings, styles, pending_cmd)
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

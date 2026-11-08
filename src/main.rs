use std::io;

use anyhow::Result;
use app::ui::themes::ThemeStore;
use clap::Parser;
use console_backend::ConsoleBackend;
use ratatui::Terminal;
use settings::Settings;

mod app;
mod cli;
mod console_backend;
#[cfg(test)]
mod journal_tests;
mod journals;
mod logging;
mod settings;
mod terminal_session;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = cli::Cli::parse();

    if cli.execute_backup().await? {
        return Ok(());
    }

    if matches!(cli.command, Some(cli::CliCommand::Licenses)) {
        cli::commands::print_licenses()?;
        return Ok(());
    }

    let custom_config = cli.config_path.clone();
    let show_selector = cli.shows_journal_selector();
    let mut settings = Settings::new(custom_config.clone()).await?;
    let settings_path = Settings::file_path(custom_config.clone())?;
    let launch_details = cli.launch_details();
    let override_owner = cli
        .storage_override_settings(&settings)?
        .as_ref()
        .map(journals::journal_for_storage)
        .transpose()?
        .flatten();
    let override_lease = override_owner
        .as_ref()
        .map(|(store, journal)| store.acquire(journal))
        .transpose()?;
    let override_profile = override_owner
        .as_ref()
        .map(|(_, journal)| &journal.settings_path);

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
        .map(|error| format!("Could not install the default Tui theme: {error:#}"));
    let mut loaded_theme = theme_store.resolve(settings.theme.as_deref());
    let mut details = launch_details?;
    let mut journal_name = None;
    let mut journal_lease = None;
    let mut leased_profile = None;
    let direct_journal = journals::journal_for_config(&settings_path)?;
    chain_panic_hook();
    let _terminal_session = terminal_session::TerminalSession::enter()?;
    let linux_console =
        std::env::var("TERM").is_ok_and(|term| console_backend::is_linux_console(&term));
    let backend = ConsoleBackend::new(io::stdout(), linux_console);
    let mut terminal = Terminal::new(backend)?;

    if show_selector {
        let (store, initial_journal) = match direct_journal {
            Some((store, journal)) => (store, Some(journal)),
            None => (
                journals::JournalStore::new(settings_path)?
                    .with_themes_path(theme_store.directory().to_path_buf())?,
                None,
            ),
        };
        return app::journal_startup::run_collection(
            &mut terminal,
            &store,
            &theme_store,
            initial_journal,
            loaded_theme.styles,
            details,
            initialization_warning,
        )
        .await
        .inspect_err(|error| log::error!("Journal session failed: {error:#}"));
    }

    if let Some((store, journal)) = direct_journal {
        if override_profile != Some(&journal.settings_path) {
            journal_lease = Some(store.acquire(&journal)?);
        }
        leased_profile = Some(journal.settings_path);
        details.push(format!("Journal collection: {}", store.root().display()));
        journal_name = Some(journal.name);
    }
    let storage_lease = journals::journal_for_storage(&settings)?
        .filter(|(_, journal)| {
            leased_profile.as_ref() != Some(&journal.settings_path)
                && override_profile != Some(&journal.settings_path)
        })
        .map(|(store, journal)| store.acquire(&journal))
        .transpose()?;
    if let Some(name) = &journal_name {
        details.insert(0, format!("Journal: {name}"));
    }
    if let Some(warning) = initialization_warning {
        loaded_theme.warning = Some(match loaded_theme.warning {
            Some(theme_warning) => format!("{warning}\n{theme_warning}"),
            None => warning,
        });
    }
    let config_context = settings::ConfigContext {
        journal_name,
        return_to_journals: false,
        settings_path,
        themes_path: theme_store.directory().to_path_buf(),
        active_theme_id: loaded_theme.id,
        theme_warning: loaded_theme.warning,
        launch_details: details,
    };

    let result = app::run(
        &mut terminal,
        settings,
        loaded_theme.styles,
        pending_cmd,
        config_context,
    )
    .await
    .inspect_err(|err| {
        log::error!("[PANIC] {err:?}");
    });
    drop(storage_lease);
    drop(journal_lease);
    drop(override_lease);
    result?;

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

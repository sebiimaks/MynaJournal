use anyhow::{Context, Result};
use crossterm::event::EventStream;
use futures_util::StreamExt;
use ratatui::{Terminal, backend::Backend};

use crate::{
    journals::{Journal, JournalLease, JournalStore},
    settings::{ConfigContext, Settings},
};

use super::ui::{
    Styles,
    journal_selector::{JournalChoice, JournalSelector, SelectorAction},
    themes::ThemeStore,
};

pub struct JournalSelection {
    pub journal: Journal,
    pub settings: Settings,
    pub lease: JournalLease,
    pub storage_lease: Option<JournalLease>,
}

fn open_journal(store: &JournalStore, journal: Journal) -> Result<JournalSelection> {
    let lease = store.acquire(&journal)?;
    let settings = store.load_settings(&journal)?;
    let storage_lease = crate::journals::journal_for_storage(&settings)?
        .filter(|(_, owner)| owner.settings_path != journal.settings_path)
        .map(|(store, owner)| store.acquire(&owner))
        .transpose()?;
    Ok(JournalSelection {
        journal,
        settings,
        lease,
        storage_lease,
    })
}

fn choices(store: &JournalStore, journals: &[Journal]) -> Vec<JournalChoice> {
    journals
        .iter()
        .map(|journal| JournalChoice {
            id: journal.id.clone(),
            name: journal.name.clone(),
            is_personal: journal.is_personal,
            locked: store
                .load_settings(journal)
                .and_then(|settings| crate::journal_storage::backend(&settings))
                .and_then(|backend| backend::journal::JournalProvider::status(&backend))
                .map_or(true, |status| {
                    status == backend::journal::JournalStatus::Protected
                }),
        })
        .collect()
}

pub async fn run_collection<B: Backend>(
    terminal: &mut Terminal<B>,
    store: &JournalStore,
    themes: &ThemeStore,
    mut initial_journal: Option<Journal>,
    mut selector_styles: Styles,
    launch_details: Vec<String>,
    mut initialization_warning: Option<String>,
) -> Result<()>
where
    B::Error: Send + Sync + 'static,
{
    let mut selected_id = None;
    let mut selector_error = None;
    loop {
        let selection = if let Some(journal) = initial_journal.take() {
            selected_id = Some(journal.id.clone());
            match open_journal(store, journal) {
                Ok(selection) => selection,
                Err(error) => {
                    selector_error = Some(format!("Could not open journal: {error:#}"));
                    continue;
                }
            }
        } else {
            let Some(selection) = select(
                terminal,
                store,
                &selector_styles,
                selected_id.as_deref(),
                selector_error.take(),
            )
            .await?
            else {
                return Ok(());
            };
            selection
        };
        let JournalSelection {
            journal,
            settings,
            lease,
            storage_lease,
        } = selection;
        let mut theme = themes.resolve(settings.theme.as_deref());
        if let Some(warning) = initialization_warning.take() {
            theme.warning = Some(match theme.warning {
                Some(theme_warning) => format!("{warning}\n{theme_warning}"),
                None => warning,
            });
        }
        let mut details = launch_details.clone();
        details.insert(0, format!("Journal: {}", journal.name));
        details.push(format!("Journal collection: {}", store.root().display()));
        let context = ConfigContext {
            journal_name: Some(journal.name.clone()),
            return_to_journals: true,
            storage_overridden: false,
            settings_path: journal.settings_path.clone(),
            themes_path: themes.directory().to_path_buf(),
            active_theme_id: theme.id,
            theme_warning: theme.warning,
            launch_details: details,
        };
        let outcome = super::run(terminal, settings, theme.styles, None, context).await;
        drop(storage_lease);
        drop(lease);
        selected_id = Some(journal.id.clone());
        match outcome {
            Ok(super::RunOutcome::InputEnded) => return Ok(()),
            Ok(super::RunOutcome::JournalClosed) => {}
            Err(error) => selector_error = Some(format!("Could not open journal: {error:#}")),
        }
        if let Ok(settings) = store.load_settings(&journal) {
            selector_styles = themes.resolve(settings.theme.as_deref()).styles;
        }
    }
}

async fn select<B: Backend>(
    terminal: &mut Terminal<B>,
    store: &JournalStore,
    styles: &Styles,
    preferred_id: Option<&str>,
    error: Option<String>,
) -> Result<Option<JournalSelection>>
where
    B::Error: Send + Sync + 'static,
{
    crate::terminal_session::clear_fullscreen(terminal)?;
    let mut journals = store.list()?;
    let mut selector = JournalSelector::new(choices(store, &journals));
    if preferred_id.is_some() {
        selector.replace_journals(choices(store, &journals), preferred_id);
    }
    if let Some(error) = error {
        selector.set_error(error);
    }
    // The editor consumes ordinary key events; only the selector handles Paste events.
    let _paste = crate::terminal_session::BracketedPasteMode::enter()?;
    let mut events = EventStream::new();
    loop {
        terminal.draw(|frame| selector.render(frame, frame.area(), styles))?;
        let Some(event) = events.next().await else {
            return Ok(None);
        };
        let action = selector.handle_event(event.context("Reading journal selector input")?);
        let result: Result<()> = match action {
            SelectorAction::None => continue,
            SelectorAction::Quit => return Ok(None),
            SelectorAction::Open(id) => {
                let selected = (|| {
                    let journal = journals
                        .iter()
                        .find(|journal| journal.id == id)
                        .context("The selected journal is no longer available")?
                        .clone();
                    open_journal(store, journal)
                })();
                match selected {
                    Ok(selection) => return Ok(Some(selection)),
                    Err(error) => Err(error),
                }
            }
            SelectorAction::Create(name) => (|| {
                let personal = journals
                    .iter()
                    .find(|journal| journal.is_personal)
                    .context("Personal journal is missing")?;
                let base = store.load_settings(personal)?;
                let journal = store.create(&name, &base)?;
                journals = store.list()?;
                selector.replace_journals(choices(store, &journals), Some(&journal.id));
                selector.set_notice(format!("Created {}. Press Enter to open.", journal.name));
                Ok(())
            })(),
            SelectorAction::Delete(id) => (|| {
                let journal = journals
                    .iter()
                    .find(|journal| journal.id == id)
                    .context("The selected journal is no longer available")?;
                let name = journal.name.clone();
                store.archive(journal)?;
                journals = store.list()?;
                selector.replace_journals(choices(store, &journals), None);
                selector.set_notice(format!("Moved {name} to Deleted Journals."));
                Ok(())
            })(),
        };
        if let Err(error) = result {
            selector.set_error(format!("{error:#}"));
        }
    }
}

#[cfg(all(test, feature = "json"))]
mod tests {
    use super::*;
    use crate::settings::BackendType;

    #[tokio::test]
    async fn selector_lock_status_refreshes_after_changes_and_fails_closed_on_corrupt_metadata() {
        use backend::{DataProvider, journal::JournalProvider};
        let temporary = tempfile::tempdir().unwrap();
        let store = JournalStore::new(temporary.path().join("config.toml")).unwrap();
        let base = Settings {
            backend_type: Some(BackendType::Json),
            ..Default::default()
        };
        let journal = store.create("Work", &base).unwrap();
        let journals = [journal.clone()];
        assert!(!choices(&store, &journals)[0].locked);
        let settings = store.load_settings(&journal).unwrap();
        let backend = crate::journal_storage::backend(&settings).unwrap();
        let mut provider = JournalProvider::open(backend.clone()).await.unwrap();
        provider
            .enable_journal_protection("journal test password".to_owned().into(), None)
            .await
            .unwrap();
        assert!(choices(&store, &journals)[0].locked);
        provider
            .disable_journal_protection("journal test password".to_owned().into())
            .await
            .unwrap();
        assert!(!choices(&store, &journals)[0].locked);
        drop(provider);
        std::fs::write(backend.header_path(), "broken header").unwrap();
        assert!(choices(&store, &journals)[0].locked);
        std::fs::write(&journal.settings_path, "invalid = [").unwrap();
        assert!(choices(&store, &journals)[0].locked);
    }

    #[test]
    fn opening_redirected_storage_holds_both_leases_and_releases_them_on_failure_or_close() {
        let temporary = tempfile::tempdir().unwrap();
        let store = JournalStore::new(temporary.path().join("config.toml")).unwrap();
        let base = Settings {
            backend_type: Some(BackendType::Json),
            ..Default::default()
        };
        let profile = store.create("Work", &base).unwrap();
        let owner = store.create("Storage owner", &base).unwrap();
        let owner_settings = store.load_settings(&owner).unwrap();
        let mut redirected = store.load_settings(&profile).unwrap();
        redirected.json_backend.file_path = owner_settings.json_backend.file_path;
        std::fs::write(
            &profile.settings_path,
            toml::to_string(&redirected).unwrap(),
        )
        .unwrap();

        let busy_owner = store.acquire(&owner).unwrap();
        let error = open_journal(&store, profile.clone()).err().unwrap();
        assert!(error.to_string().contains("Storage owner"));
        assert!(error.to_string().contains("open in another window"));
        drop(store.acquire(&profile).unwrap());
        assert!(store.acquire(&owner).is_err());
        drop(busy_owner);

        let selection = open_journal(&store, profile.clone()).unwrap();
        assert_eq!(selection.journal, profile);
        assert_eq!(
            selection.settings.json_backend.file_path,
            redirected.json_backend.file_path
        );
        assert!(selection.storage_lease.is_some());
        assert!(store.acquire(&profile).is_err());
        assert!(store.acquire(&owner).is_err());
        drop(selection);

        let profile_lease = store.acquire(&profile).unwrap();
        let owner_lease = store.acquire(&owner).unwrap();
        drop((profile_lease, owner_lease));
    }
}

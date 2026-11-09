use anyhow::{Context, Result, ensure};
use backend::DataProvider;

use crate::settings::{Settings, document::ConfigDocument};

use super::journal_security_popup::{JournalPasswordAction, JournalSecurityPopup};
use super::themes::ThemeStore;
use super::{App, ConfigAction, ConfigPopup, Popup, Styles, UIComponents};

impl UIComponents<'_> {
    pub fn show_config<D: DataProvider>(&mut self, app: &App<D>) -> Result<()> {
        let context = app
            .config_context
            .as_ref()
            .context("Configuration paths are unavailable")?;
        let settings = ConfigDocument::load(context.settings_path.clone())?;
        let themes = ThemeStore::from_directory(context.themes_path.clone())?;
        let mut popup = ConfigPopup::new(
            settings,
            themes,
            &app.settings,
            &self.styles,
            self.active_theme_id.as_deref(),
            app.state.entries_list_percentage(),
        )?;
        popup.set_launch_details(context.launch_details.clone());
        popup.set_journal_protected(app.journal_is_protected());
        self.cancel_divider_drag();
        self.popup_stack.push(Popup::Config(Box::new(popup)));
        Ok(())
    }

    pub fn save_config<D: DataProvider>(
        &mut self,
        action: ConfigAction,
        app: &mut App<D>,
    ) -> Result<()> {
        let security_action = match action {
            ConfigAction::EnableJournalProtection => Some(JournalPasswordAction::Enable),
            ConfigAction::ChangeJournalPassword => Some(JournalPasswordAction::Change),
            ConfigAction::DisableJournalProtection => Some(JournalPasswordAction::Disable),
            _ => None,
        };
        if let Some(action) = security_action {
            ensure!(
                !self.has_unsaved()
                    && !self
                        .popup_stack
                        .iter()
                        .any(|popup| matches!(popup, Popup::Entry(_))),
                "Save or discard entry edits before changing journal protection"
            );
            ensure!(
                action != JournalPasswordAction::Enable || !app.has_sealed_drafts(),
                "Unlock and save or discard all encrypted entry drafts before protecting the journal"
            );
            self.popup_stack
                .push(Popup::JournalSecurity(Box::new(JournalSecurityPopup::new(
                    action,
                ))));
            return Ok(());
        }
        let Some(Popup::Config(popup)) = self.popup_stack.last_mut() else {
            return Ok(());
        };
        match action {
            ConfigAction::SaveSettings => {
                let document = popup.settings_document_mut();
                let saved = Settings::from_config_text(&document.text(), document.path())
                    .context("Invalid application settings")?;
                document.save()?;
                app.apply_live_settings(saved);
                self.editor.set_word_wrap(app.settings.editor_word_wrap);
                popup.mark_settings_saved(&app.settings);
            }
            ConfigAction::SaveAppearance => {
                let is_active = popup.theme_is_active();
                let document = popup.themes_document_mut()?;
                let styles =
                    Styles::deserialize(&document.text()).context("Invalid appearance settings")?;
                document.save()?;
                if is_active {
                    app.set_tag_palette(&styles.tags.palette);
                    self.styles = styles.clone();
                }
                popup.mark_appearance_saved(&styles);
            }
            ConfigAction::ActivateTheme => {
                let (id, styles) = popup.selected_theme()?;
                popup
                    .settings_document_mut()
                    .save_field_preserving_draft(&["theme"], Some(id.clone().into()))?;
                app.settings.theme = Some(id.clone());
                self.active_theme_id = Some(id.clone());
                app.set_tag_palette(&styles.tags.palette);
                self.styles = styles;
                popup.mark_theme_activated(&id);
            }
            ConfigAction::SaveLayout(percentage) => {
                let previous = app.state.entries_list_percentage();
                app.state.set_entries_list_percentage(percentage);
                if let Err(error) = app.persist_state() {
                    app.state.set_entries_list_percentage(previous);
                    return Err(error.context("Could not save the divider position"));
                }
                popup.mark_layout_saved(app.state.entries_list_percentage());
            }
            ConfigAction::Keep
            | ConfigAction::Close
            | ConfigAction::EnableJournalProtection
            | ConfigAction::ChangeJournalPassword
            | ConfigAction::DisableJournalProtection => {}
        }
        Ok(())
    }
}

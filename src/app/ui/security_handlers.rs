use super::security_popup::{PasswordAction, SecurityAction, SecurityPopup};
use super::{
    App, ControlType, Editor, EditorMode, HandleInputReturnType, MsgBoxActions, MsgBoxResult,
    MsgBoxType, Popup, UICommand, UIComponents,
};
use crate::app::keymap::Input;
use anyhow::{Result, ensure};
use backend::DataProvider;

impl UIComponents<'_> {
    pub(super) fn show_security<D: DataProvider>(
        &mut self,
        app: &App<D>,
        unlock: bool,
    ) -> Result<()> {
        let id = app
            .current_entry_id
            .ok_or_else(|| anyhow::anyhow!("Select a note first"))?;
        let mut popup = SecurityPopup::new(
            id,
            app.is_protected(id),
            app.is_locked(id),
            app.has_sealed_draft(id),
        );
        if unlock {
            popup.prompt(PasswordAction::Unlock);
        } else if !app.is_protected(id) {
            popup.prompt(PasswordAction::Protect);
        }
        self.popup_stack.push(Popup::Security(Box::new(popup)));
        Ok(())
    }

    pub(super) async fn handle_security_input<D: DataProvider>(
        &mut self,
        input: &Input,
        app: &mut App<D>,
    ) -> Result<HandleInputReturnType> {
        let (id, action) = match self.popup_stack.last_mut() {
            Some(Popup::Security(popup)) => (popup.entry_id, popup.handle_input(input)),
            _ => unreachable!(),
        };
        let result: Result<bool> = async {
            match action {
                SecurityAction::Keep => return Ok(false),
                SecurityAction::Close => {}
                SecurityAction::Protect(password) => {
                    app.protect_note(id, password, self.editor.get_content())
                        .await?;
                    self.reset_private_views(app);
                    return Ok(false);
                }
                SecurityAction::Unlock(password) => {
                    let draft = app.unlock_note(id, password).await?;
                    self.popup_stack.pop();
                    if self.privacy_shield {
                        // A failed idle lock kept the editor in memory behind the shield.
                        self.privacy_shield = false;
                        self.popup_stack = std::mem::take(&mut self.shielded_popups);
                    } else {
                        self.editor.set_current_entry(Some(id), app);
                        if let Some(draft) = draft {
                            self.editor.set_entry_content(&draft.content, app);
                            if let Some(snapshot) = &draft.entry_popup {
                                self.popup_stack.push(Popup::Entry(Box::new(
                                    super::EntryPopup::from_snapshot(snapshot)?,
                                )));
                            }
                        }
                    }
                    self.change_active_control(ControlType::EntryContentTxt);
                    self.editor.set_editor_mode(EditorMode::Normal);
                    return Ok(false);
                }
                SecurityAction::Lock => {
                    self.popup_stack.pop();
                    if self.has_unsaved() {
                        self.show_unsaved_msg_box(Some(UICommand::LockNote));
                    } else {
                        app.lock_note(id);
                        self.reset_private_views(app);
                    }
                    return Ok(false);
                }
                SecurityAction::Change(password) => {
                    ensure!(
                        !self.has_unsaved(),
                        "Save or discard your edits before changing the password"
                    );
                    app.change_note_password(id, password).await?;
                }
                SecurityAction::Remove => {
                    ensure!(
                        !self.has_unsaved(),
                        "Save or discard your edits before removing protection"
                    );
                    app.remove_note_protection(id).await?;
                    self.reset_private_views(app);
                    return Ok(false);
                }
                SecurityAction::DiscardDraft => app.discard_sealed_draft(id),
            }
            Ok(true)
        }
        .await;
        match result {
            Ok(true) => {
                self.popup_stack.pop();
            }
            Ok(false) => {}
            Err(error) => {
                if let Some(Popup::Security(popup)) = self.popup_stack.last_mut() {
                    popup.show_error(error.to_string());
                } else {
                    return Err(error);
                }
            }
        }
        Ok(HandleInputReturnType::Handled)
    }

    pub(super) fn reset_private_views<D: DataProvider>(&mut self, app: &App<D>) {
        self.privacy_shield = false;
        self.shielded_popups.clear();
        self.popup_stack.clear();
        self.pending_command = None;
        self.editor = Editor::new();
        self.editor.set_current_entry(app.current_entry_id, app);
        self.active_control = ControlType::EntriesList;
        self.entries_list.set_active(true);
        self.cancel_divider_drag();
    }

    pub async fn auto_lock<D: DataProvider>(&mut self, app: &mut App<D>) -> Result<()> {
        let entry_popup = if app.current_is_protected() && !app.current_is_locked() {
            self.popup_stack
                .iter()
                .find_map(|popup| match popup {
                    Popup::Entry(popup) => Some(popup.snapshot()),
                    _ => None,
                })
                .transpose()?
        } else {
            None
        };
        if let Some(id) = app.current_entry_id
            && app.current_is_protected()
            && !app.current_is_locked()
            && (self.has_unsaved() || entry_popup.is_some())
        {
            app.seal_draft_and_lock(id, self.editor.get_content(), entry_popup)
                .await?;
        }
        app.lock_all_saved_notes();
        if app.current_is_protected() {
            let configuration: Vec<_> = self
                .popup_stack
                .drain(..)
                .filter(|popup| matches!(popup, Popup::Config(_)))
                .collect();
            self.reset_private_views(app);
            self.popup_stack = configuration;
        } else {
            self.privacy_shield = false;
        }
        Ok(())
    }

    pub fn is_privacy_shielded(&self) -> bool {
        self.privacy_shield
    }

    pub fn hide_for_auto_lock(&mut self) {
        self.privacy_shield = true;
        self.cancel_divider_drag();
    }

    pub fn recover_failed_auto_lock<D: DataProvider>(
        &mut self,
        app: &mut App<D>,
        error: &anyhow::Error,
    ) -> Result<()> {
        self.shielded_popups = std::mem::take(&mut self.popup_stack);
        self.pending_command = None;
        let current = app.current_entry_id;
        let other_ids: Vec<_> = app
            .security
            .sessions
            .keys()
            .copied()
            .filter(|id| Some(*id) != current)
            .collect();
        for id in other_ids {
            app.lock_note(id);
        }
        app.record_activity();
        self.show_security(app, true)?;
        if let Some(Popup::Security(popup)) = self.popup_stack.last_mut() {
            popup.show_error(format!("Could not seal your unsaved edits: {error}. Draft kept in memory; unlock to recover."));
        }
        Ok(())
    }

    pub(super) async fn continue_lock_note<D: DataProvider>(
        &mut self,
        app: &mut App<D>,
        choice: MsgBoxResult,
    ) -> Result<HandleInputReturnType> {
        if matches!(choice, MsgBoxResult::Yes | MsgBoxResult::No) {
            if choice == MsgBoxResult::Yes {
                app.update_current_entry_content(self.editor.get_content())
                    .await?;
            }
            if let Some(id) = app.current_entry_id {
                app.lock_note(id);
            }
            self.reset_private_views(app);
        }
        Ok(HandleInputReturnType::Handled)
    }

    pub(super) fn confirm_private_clipboard(
        &mut self,
        command: UICommand,
    ) -> HandleInputReturnType {
        self.show_msg_box(MsgBoxType::Question("Copy private text to the system clipboard? It can remain in clipboard history after this note is locked.".into()), MsgBoxActions::YesNo, Some(command));
        HandleInputReturnType::Handled
    }
}

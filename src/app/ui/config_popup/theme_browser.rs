use super::*;

impl ConfigPopup {
    pub fn themes_document_mut(&mut self) -> Result<&mut ThemeDocument> {
        self.themes_document
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Open a theme for editing first."))
    }

    pub fn edited_theme_id(&self) -> Option<&str> {
        self.themes_document.as_ref().map(ThemeDocument::id)
    }

    pub fn theme_is_active(&self) -> bool {
        self.active_theme_id
            .as_deref()
            .is_some_and(|active| self.edited_theme_id() == Some(active))
    }

    pub fn selected_theme(&self) -> Result<(String, Styles)> {
        let theme = self
            .theme_catalog
            .get(self.theme_selected)
            .ok_or_else(|| anyhow::anyhow!("Select a theme first."))?;
        let document = self.theme_store.load(&theme.id)?;
        Ok((theme.id.clone(), Styles::deserialize(&document.text())?))
    }

    pub fn mark_theme_activated(&mut self, id: &str) {
        self.active_theme_id = Some(id.into());
        if let Ok(settings) = toml::from_str::<Settings>(&self.settings_document.text())
            && let Ok(value) = settings_value(&settings)
        {
            self.settings = value;
        }
        if let Ok(value) = toml::from_str(&self.settings_document.text()) {
            self.raw_settings = value;
        }
        if let Some(table) = self.active_settings.as_table_mut() {
            table.insert("theme".into(), Value::String(id.into()));
        }
        self.status = "Theme activated. Other draft changes are kept.".into();
        self.error = false;
    }

    pub(super) fn theme_dirty(&self) -> bool {
        self.themes_document
            .as_ref()
            .is_some_and(ThemeDocument::is_dirty)
    }

    pub(super) fn open_theme(&mut self, document: ThemeDocument) -> Result<()> {
        let themes = Value::try_from(Styles::deserialize(&document.text())?)?;
        self.raw_themes = toml::from_str(&document.text())?;
        self.saved_themes = themes.clone();
        self.themes = themes;
        self.themes_document = Some(document);
        self.selected[self.tab] = 0;
        self.table_state = TableState::default();
        self.status.clear();
        self.error = false;
        Ok(())
    }

    pub(super) fn edit_selected_theme(&mut self) {
        let result = self
            .theme_catalog
            .get(self.theme_selected)
            .ok_or_else(|| anyhow::anyhow!("Select a theme first."))
            .and_then(|theme| self.theme_store.load(&theme.id))
            .and_then(|document| self.open_theme(document));
        if let Err(error) = result {
            self.show_error(error.to_string());
        }
    }

    pub(super) fn begin_new_theme(&mut self) {
        let Some(theme) = self.theme_catalog.get(self.theme_selected) else {
            return;
        };
        if let Err(error) = self.theme_store.load(&theme.id) {
            self.show_error(error.to_string());
            return;
        }
        self.creating_theme = true;
        self.editor = Some(TextArea::default());
        self.status = "Name the new theme copied from the selected theme.".into();
        self.error = false;
    }

    pub(super) fn leave_theme_editor(&mut self) {
        self.themes_document = None;
        self.confirm_theme_close = false;
        self.table_state = TableState::default();
        self.status.clear();
        self.error = false;
    }

    pub fn mark_appearance_saved(&mut self, saved: &Styles) {
        if let Ok(value) = Value::try_from(saved) {
            self.saved_themes = value.clone();
            self.themes = value;
        }
        if let Some(document) = &self.themes_document
            && let Ok(value) = toml::from_str(&document.text())
        {
            self.raw_themes = value;
        }
        let id = self.edited_theme_id().map(str::to_owned);
        match self.theme_store.list() {
            Ok(catalog) => {
                self.theme_selected = catalog
                    .iter()
                    .position(|theme| Some(&theme.id) == id.as_ref())
                    .unwrap_or(0);
                self.theme_catalog = catalog;
            }
            Err(error) => {
                self.show_error(format!(
                    "Theme saved, but the theme list could not be refreshed: {error}"
                ));
                return;
            }
        }
        self.status = if self.theme_is_active() {
            "Theme saved and applied."
        } else {
            "Theme saved. Activate it from the theme list to apply it."
        }
        .into();
        self.error = false;
    }

    pub(super) fn render_theme_list(&mut self, frame: &mut Frame, area: Rect, styles: &Styles) {
        let compact = area.height < 14;
        let chunks = Layout::vertical([
            Constraint::Length(if compact { 1 } else { 3 }),
            Constraint::Length(u16::from(!compact)),
            Constraint::Min(2),
            Constraint::Length(if self.creating_theme { 3 } else { 0 }),
            Constraint::Length(if compact { 1 } else { 2 }),
            Constraint::Length(if area.width < 66 { 3 } else { 2 }),
        ])
        .split(area);
        self.render_tabs(frame, chunks[0]);
        frame.render_widget(
            Paragraph::new(safe_text(&format!(
                "Themes: {}",
                self.theme_store.directory().display()
            ))),
            chunks[1],
        );
        let rows: Vec<_> = self
            .theme_catalog
            .iter()
            .map(|theme| {
                let status = if theme.error.is_some() {
                    "Invalid"
                } else if Some(theme.id.as_str()) == self.active_theme_id.as_deref() {
                    "Active"
                } else {
                    ""
                };
                Row::new([safe_text(&theme.label), status.into()])
            })
            .collect();
        self.table_state.select(Some(self.theme_selected));
        self.page_size = chunks[2].height.saturating_sub(1).max(1) as usize;
        frame.render_stateful_widget(
            Table::new(
                rows,
                [Constraint::Percentage(75), Constraint::Percentage(25)],
            )
            .header(Row::new(["Theme", "Status"]).style(Style::default().fg(Color::LightBlue)))
            .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
            chunks[2],
            &mut self.table_state,
        );
        if let Some(editor) = &mut self.editor {
            editor.set_block(
                Block::default()
                    .title("New theme name · Enter accepts · Esc cancels")
                    .borders(Borders::ALL),
            );
            editor.set_cursor_line_style(Style::default());
            editor.set_style(Style::from(if self.error {
                styles.general.input_block_invalid
            } else {
                styles.general.input_block_active
            }));
            editor.set_cursor_style(Style::from(styles.general.input_cursor_active));
            frame.render_widget(&*editor, chunks[3]);
        }
        let selected_error = self
            .theme_catalog
            .get(self.theme_selected)
            .and_then(|theme| theme.error.as_deref());
        let status = if self.confirm_close && (compact || area.width < 45) {
            "Discard? y/n"
        } else if self.confirm_close {
            "Discard all unsaved changes? y/n"
        } else if !self.status.is_empty() {
            &self.status
        } else if let Some(error) = selected_error {
            error
        } else if self.active_theme_id.is_none() {
            "Built-in Tui fallback active. Activate a listed theme to select it."
        } else {
            "Select a theme to activate, edit or duplicate."
        };
        frame.render_widget(
            Paragraph::new(safe_text(status))
                .wrap(Wrap { trim: false })
                .style(
                    Style::default().fg(if self.error || selected_error.is_some() {
                        styles.msgbox.error
                    } else {
                        styles.msgbox.info
                    }),
                ),
            chunks[4],
        );
        let footer = if self.creating_theme && area.width < 30 {
            "Enter Accept\nEsc Cancel ^S Save"
        } else if self.creating_theme {
            "Enter: create draft  Esc: cancel\nCtrl-S: create and save"
        } else if area.width < 30 {
            "a Activate e Edit\nn New ↑↓ Select\nTab Next Esc Close"
        } else if area.width < 66 {
            "Enter/a Activate  e Edit\nn New/duplicate  ↑↓ Select\nTab Sections  Esc Close"
        } else {
            "↑↓/PgUp/PgDn: themes  Enter/a: activate  e: edit  n: new/duplicate\nTab/←→: sections  Esc/q: close"
        };
        frame.render_widget(Paragraph::new(footer).wrap(Wrap { trim: false }), chunks[5]);
    }
}

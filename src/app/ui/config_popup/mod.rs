mod fields;
mod theme_browser;

use anyhow::{Result, bail, ensure};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout, Margin, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState, Tabs, Wrap},
};
use toml::Value;
use tui_textarea::TextArea;

use crate::{
    app::keymap::Input,
    settings::{Settings, document::ConfigDocument},
};

use super::{
    themes::{Styles, TagColors, ThemeDocument, ThemeInfo, ThemeStore},
    ui_functions::centered_rect,
};
use fields::{Field, Kind, Tab};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConfigAction {
    Keep,
    Close,
    SaveSettings,
    SaveAppearance,
    ActivateTheme,
    SaveLayout(u16),
}

#[derive(Debug)]
pub struct ConfigPopup {
    settings_document: ConfigDocument,
    themes_document: Option<ThemeDocument>,
    theme_store: ThemeStore,
    theme_catalog: Vec<ThemeInfo>,
    theme_selected: usize,
    active_theme_id: Option<String>,
    saved_themes: Value,
    creating_theme: bool,
    confirm_theme_close: bool,
    settings: Value,
    themes: Value,
    raw_settings: Value,
    raw_themes: Value,
    active_settings: Value,
    default_settings: Value,
    default_themes: Value,
    fields: Vec<Field>,
    tab: usize,
    selected: [usize; 7],
    table_state: TableState,
    editor: Option<TextArea<'static>>,
    divider: u16,
    saved_divider: u16,
    confirm_close: bool,
    status: String,
    error: bool,
    launch_details: Vec<String>,
    page_size: usize,
    context_scroll: usize,
}

impl ConfigPopup {
    pub fn new(
        settings_document: ConfigDocument,
        theme_store: ThemeStore,
        active_settings: &Settings,
        active_styles: &Styles,
        active_theme_id: Option<&str>,
        divider: u16,
    ) -> Result<Self> {
        let settings = settings_value(&toml::from_str(&settings_document.text())?)?;
        let themes = Value::try_from(active_styles)?;
        let default_themes = Value::try_from(Styles::default())?;
        let fields = fields::fields(&default_themes);
        let raw_settings = toml::from_str(&settings_document.text())?;
        let raw_themes = Value::Table(Default::default());
        let (theme_catalog, catalog_error) = match theme_store.list() {
            Ok(themes) => (themes, None),
            Err(error) => {
                let message = format!("Could not list themes: {error:#}");
                (
                    vec![ThemeInfo {
                        id: "tui".into(),
                        label: "Tui (default)".into(),
                        error: Some(message.clone()),
                    }],
                    Some(message),
                )
            }
        };
        let theme_selected = theme_catalog
            .iter()
            .position(|theme| Some(theme.id.as_str()) == active_theme_id)
            .unwrap_or(0);
        Ok(Self {
            settings_document,
            themes_document: None,
            theme_store,
            theme_catalog,
            theme_selected,
            active_theme_id: active_theme_id.map(str::to_owned),
            saved_themes: themes.clone(),
            creating_theme: false,
            confirm_theme_close: false,
            settings,
            themes,
            raw_settings,
            raw_themes,
            active_settings: settings_value(active_settings)?,
            default_settings: settings_value(&Settings::default())?,
            default_themes,
            fields,
            tab: 0,
            selected: [0; 7],
            table_state: TableState::default(),
            editor: None,
            divider,
            saved_divider: divider,
            confirm_close: false,
            error: catalog_error.is_some(),
            status: catalog_error.unwrap_or_default(),
            launch_details: Vec::new(),
            page_size: 5,
            context_scroll: 0,
        })
    }

    pub fn settings_document_mut(&mut self) -> &mut ConfigDocument {
        &mut self.settings_document
    }

    pub fn set_launch_details(&mut self, details: Vec<String>) {
        self.launch_details = details;
    }

    pub fn show_error(&mut self, message: String) {
        self.status = message;
        self.error = true;
    }

    pub fn mark_settings_saved(&mut self, active: &Settings) {
        match settings_value(active) {
            Ok(value) => self.active_settings = value,
            Err(error) => {
                self.show_error(error.to_string());
                return;
            }
        }
        if let Ok(settings) = toml::from_str::<Settings>(&self.settings_document.text())
            && let Ok(value) = settings_value(&settings)
        {
            self.settings = value;
        }
        if let Ok(value) = toml::from_str(&self.settings_document.text()) {
            self.raw_settings = value;
        }
        let pending = self.fields.iter().any(|field| {
            field.restart
                && field_value(field, &self.settings) != field_value(field, &self.active_settings)
        });
        self.status = if pending {
            "Settings saved. Restart to apply pending storage/history changes."
        } else {
            "Settings saved."
        }
        .into();
        self.error = false;
    }

    pub fn mark_layout_saved(&mut self, divider: u16) {
        self.divider = divider;
        self.saved_divider = divider;
        self.status = "Layout saved to app state.".into();
        self.error = false;
    }

    fn current_tab(&self) -> Tab {
        Tab::ALL[self.tab]
    }

    fn tab_fields(&self) -> impl Iterator<Item = &Field> {
        self.fields
            .iter()
            .filter(|field| field.tab == self.current_tab())
    }

    fn current_field(&self) -> &Field {
        self.tab_fields()
            .nth(self.selected[self.tab])
            .expect("each configuration tab has a selected field")
    }

    fn value(&self, field: &Field) -> Option<Value> {
        if field.tab == Tab::Layout {
            Some(Value::Integer(i64::from(self.divider)))
        } else {
            field_value(
                field,
                if field.tab == Tab::Appearance {
                    &self.themes
                } else {
                    &self.settings
                },
            )
        }
    }

    fn active_value(&self, field: &Field) -> Option<Value> {
        if field.tab == Tab::Layout {
            Some(Value::Integer(i64::from(self.saved_divider)))
        } else {
            field_value(
                field,
                if field.tab == Tab::Appearance {
                    &self.saved_themes
                } else {
                    &self.active_settings
                },
            )
        }
    }

    fn default_value(&self, field: &Field) -> Option<Value> {
        if field.tab == Tab::Layout {
            Some(Value::Integer(30))
        } else {
            field_value(
                field,
                if field.tab == Tab::Appearance {
                    &self.default_themes
                } else {
                    &self.default_settings
                },
            )
        }
    }

    fn raw_value(&self, field: &Field) -> Option<Value> {
        let values = if field.tab == Tab::Appearance {
            &self.raw_themes
        } else {
            &self.raw_settings
        };
        let keys = field.keys();
        nested(values, &keys).cloned().or_else(|| {
            if keys.first() == Some(&"external_editor") && keys.get(1) == Some(&"command") {
                nested(values, &["external_editor"])
                    .filter(|value| value.is_str())
                    .cloned()
            } else if keys.first() == Some(&"general") {
                let alias = match keys.get(1).copied() {
                    Some("input_cursor_active") => "input_corsur_active",
                    Some("input_cursor_invalid") => "input_corsur_invalid",
                    _ => return None,
                };
                nested(values, &["general", alias, keys[2]]).cloned()
            } else {
                None
            }
        })
    }

    fn is_dirty(&self) -> bool {
        self.settings_document.is_dirty()
            || self.theme_dirty()
            || self.divider != self.saved_divider
    }

    pub fn handle_input(&mut self, input: &Input) -> ConfigAction {
        let ctrl = input.modifiers.contains(KeyModifiers::CONTROL);
        if self.confirm_theme_close {
            match input.key_code {
                KeyCode::Char('y' | 'Y') => self.leave_theme_editor(),
                KeyCode::Esc | KeyCode::Char('n' | 'N') => self.confirm_theme_close = false,
                _ => {}
            }
            return ConfigAction::Keep;
        }
        if self.confirm_close {
            return match input.key_code {
                KeyCode::Char('y' | 'Y') => ConfigAction::Close,
                KeyCode::Esc | KeyCode::Char('n' | 'N') => {
                    self.confirm_close = false;
                    ConfigAction::Keep
                }
                _ => ConfigAction::Keep,
            };
        }
        if self.editor.is_some() {
            match input.key_code {
                KeyCode::Esc => self.cancel_edit(),
                KeyCode::Char('c') if ctrl => self.cancel_edit(),
                KeyCode::Enter => {
                    self.commit_edit();
                }
                KeyCode::Char('s') if ctrl => {
                    if self.commit_edit() {
                        return self.save_action();
                    }
                }
                KeyCode::Char('j' | 'm') if ctrl => {}
                KeyCode::Char(ch) if ch.is_control() => {}
                _ => {
                    if let Some(editor) = self.editor.as_mut() {
                        editor.input(KeyEvent::from(input));
                    }
                }
            }
            return ConfigAction::Keep;
        }
        if ctrl && input.key_code == KeyCode::Char('s') {
            return self.save_action();
        }
        if input.key_code == KeyCode::Esc || (ctrl && input.key_code == KeyCode::Char('c')) {
            if self.current_tab() == Tab::Appearance && self.themes_document.is_some() {
                if self.theme_dirty() {
                    self.confirm_theme_close = true;
                } else {
                    self.leave_theme_editor();
                }
                return ConfigAction::Keep;
            }
            return self.request_close();
        }
        if input
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            return ConfigAction::Keep;
        }
        if self.current_tab() == Tab::Appearance && self.themes_document.is_none() {
            match input.key_code {
                KeyCode::Enter | KeyCode::Char('a') => return ConfigAction::ActivateTheme,
                KeyCode::Char('e') => self.edit_selected_theme(),
                KeyCode::Char('n') => self.begin_new_theme(),
                KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
                KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
                KeyCode::PageDown => self.move_selection(self.page_size as isize),
                KeyCode::PageUp => self.move_selection(-(self.page_size as isize)),
                KeyCode::Home => self.theme_selected = 0,
                KeyCode::End => self.theme_selected = self.theme_catalog.len().saturating_sub(1),
                KeyCode::Tab | KeyCode::Right => self.change_tab(1),
                KeyCode::BackTab | KeyCode::Left => self.change_tab(-1),
                KeyCode::Char('q' | '!') => return self.request_close(),
                _ => {}
            }
            return ConfigAction::Keep;
        }
        match input.key_code {
            KeyCode::Char('q' | '!') => return self.request_close(),
            KeyCode::Tab | KeyCode::Right => self.change_tab(1),
            KeyCode::BackTab | KeyCode::Left => self.change_tab(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::PageDown => self.move_selection(self.page_size as isize),
            KeyCode::PageUp => self.move_selection(-(self.page_size as isize)),
            KeyCode::Home if self.current_tab() == Tab::Context => self.context_scroll = 0,
            KeyCode::End if self.current_tab() == Tab::Context => self.context_scroll = usize::MAX,
            KeyCode::Enter | KeyCode::Char(' ' | 'd') if self.current_tab() == Tab::Context => {}
            KeyCode::Home => self.selected[self.tab] = 0,
            KeyCode::End => self.selected[self.tab] = self.tab_fields().count().saturating_sub(1),
            KeyCode::Enter | KeyCode::Char(' ') => self.begin_edit(),
            KeyCode::Char('d') => self.restore_default(),
            _ => {}
        }
        ConfigAction::Keep
    }

    fn request_close(&mut self) -> ConfigAction {
        if self.is_dirty() {
            self.confirm_close = true;
            ConfigAction::Keep
        } else {
            ConfigAction::Close
        }
    }

    fn save_action(&self) -> ConfigAction {
        match self.current_tab() {
            Tab::Appearance if self.themes_document.is_some() => ConfigAction::SaveAppearance,
            Tab::Appearance => ConfigAction::Keep,
            Tab::Layout => ConfigAction::SaveLayout(self.divider),
            Tab::Context => ConfigAction::Keep,
            _ => ConfigAction::SaveSettings,
        }
    }

    fn render_tabs(&self, frame: &mut Frame, area: Rect) {
        let initial_style = Style::default()
            .fg(Color::LightGreen)
            .add_modifier(Modifier::BOLD);
        let titles: Vec<Line> = Tab::ALL
            .iter()
            .map(|tab| {
                let dirty = match tab {
                    Tab::Appearance => self.theme_dirty(),
                    Tab::Layout => self.divider != self.saved_divider,
                    Tab::Context => false,
                    _ => self.settings_document.is_dirty(),
                };
                let (initial, rest) = tab.title().split_at(1);
                Line::from(vec![
                    Span::styled(initial, initial_style),
                    Span::raw(rest),
                    Span::raw(if dirty { "*" } else { "" }),
                ])
            })
            .collect();
        let highlight = Style::default().add_modifier(Modifier::UNDERLINED);
        let block = if area.height >= 3 {
            Block::default().borders(Borders::ALL).title("Tabs")
        } else {
            Block::default()
        };
        let width = titles.iter().map(|title| title.width() + 3).sum::<usize>();
        if width > block.inner(area).width as usize {
            let mut spans = vec![Span::raw(format!("‹ {}/{} ", self.tab + 1, Tab::ALL.len()))];
            spans.extend(titles[self.tab].spans.iter().cloned());
            spans.push(Span::raw(" ›"));
            frame.render_widget(
                Paragraph::new(Line::from(spans).style(highlight)).block(block),
                area,
            );
        } else {
            frame.render_widget(
                Tabs::new(titles)
                    .block(block)
                    .select(self.tab)
                    .highlight_style(highlight)
                    .divider("│"),
                area,
            );
        }
    }

    fn change_tab(&mut self, delta: isize) {
        self.tab = (self.tab as isize + delta).rem_euclid(Tab::ALL.len() as isize) as usize;
        self.table_state = TableState::default();
        self.status.clear();
        self.error = false;
    }

    fn move_selection(&mut self, delta: isize) {
        if self.current_tab() == Tab::Context {
            self.context_scroll = self.context_scroll.saturating_add_signed(delta);
            return;
        }
        if self.current_tab() == Tab::Appearance && self.themes_document.is_none() {
            self.theme_selected = self
                .theme_selected
                .saturating_add_signed(delta)
                .min(self.theme_catalog.len().saturating_sub(1));
            self.status.clear();
            self.error = false;
            return;
        }
        let max = self.tab_fields().count().saturating_sub(1);
        self.selected[self.tab] = self.selected[self.tab]
            .saturating_add_signed(delta)
            .min(max);
        self.status.clear();
        self.error = false;
    }

    fn begin_edit(&mut self) {
        let field = self.current_field().clone();
        let value = self.value(&field);
        let immediate = match &field.kind {
            Kind::Bool => Some(Value::Boolean(
                !value.as_ref().and_then(Value::as_bool).unwrap_or(false),
            )),
            Kind::Choice(options) => {
                let current = value.as_ref().and_then(Value::as_str).unwrap_or("");
                let index = options
                    .iter()
                    .position(|option| *option == current)
                    .map_or(0, |index| (index + 1) % options.len());
                options
                    .get(index)
                    .map(|option| Value::String((*option).into()))
            }
            Kind::TitleMode => {
                let next = match value.as_ref().and_then(Value::as_str) {
                    Some("None") => "Date",
                    Some("Date") => "Literal",
                    _ => "None",
                };
                Some(Value::String(next.into()))
            }
            _ => None,
        };
        if let Some(value) = immediate {
            if let Err(error) = self.apply_value(&field, Some(value)) {
                self.show_error(error.to_string());
            }
        } else {
            let text = match &field.kind {
                Kind::Color { optional: true } if value.is_none() => "inherit".into(),
                _ => editable_value(&field.kind, value.as_ref()),
            };
            if text.chars().any(char::is_control) {
                self.show_error("This stored value contains control characters and cannot be edited here. Press d to restore its default, or correct it in the configuration file.".into());
                return;
            }
            let mut editor = TextArea::new(vec![text]);
            editor.move_cursor(tui_textarea::CursorMove::End);
            self.editor = Some(editor);
            self.status.clear();
            self.error = false;
        }
    }

    fn cancel_edit(&mut self) {
        self.editor = None;
        self.status = if self.creating_theme {
            "New theme cancelled; other draft changes are kept."
        } else {
            "Field edit cancelled; other draft changes are kept."
        }
        .into();
        self.creating_theme = false;
        self.error = false;
    }

    fn commit_edit(&mut self) -> bool {
        let Some(editor) = &self.editor else {
            return true;
        };
        let input = editor.lines().join("\n");
        if self.creating_theme {
            let result = self
                .theme_catalog
                .get(self.theme_selected)
                .ok_or_else(|| anyhow::anyhow!("Select a theme first."))
                .and_then(|theme| self.theme_store.load(&theme.id))
                .and_then(|source| self.theme_store.create(&input, &source.text()))
                .and_then(|document| self.open_theme(document));
            return match result {
                Ok(()) => {
                    self.editor = None;
                    self.creating_theme = false;
                    self.status = "New theme staged. Ctrl-S saves it without activating it.".into();
                    true
                }
                Err(error) => {
                    self.show_error(error.to_string());
                    false
                }
            };
        }
        let field = self.current_field().clone();
        match parse_input(&field.kind, &input).and_then(|value| self.apply_value(&field, value)) {
            Ok(()) => {
                self.editor = None;
                true
            }
            Err(error) => {
                self.show_error(error.to_string());
                false
            }
        }
    }

    fn restore_default(&mut self) {
        let field = self.current_field().clone();
        let result = if field.tab == Tab::Layout {
            self.divider = 30;
            Ok(())
        } else if field.tab == Tab::Appearance {
            self.apply_value(&field, self.default_value(&field))
        } else {
            self.apply_value(&field, None)
        };
        if let Err(error) = result {
            self.show_error(error.to_string());
        } else {
            self.status = "Default staged. Ctrl-S saves this section.".into();
            self.error = false;
        }
    }

    fn apply_value(&mut self, field: &Field, value: Option<Value>) -> Result<()> {
        validate_path(field, value.as_ref())?;
        if field.tab == Tab::Layout {
            let divider = value
                .and_then(|value| value.as_integer())
                .unwrap_or(30)
                .try_into()?;
            ensure!(
                (10..=60).contains(&divider),
                "Pane width must be 10-60 percent."
            );
            self.divider = divider;
        } else if field.tab == Tab::Appearance {
            let mut document = self
                .themes_document
                .clone()
                .ok_or_else(|| anyhow::anyhow!("Open a theme for editing first."))?;
            if field.path.len() == 3 {
                let parent = &field.keys()[..2];
                if document.value(parent).is_none()
                    && let Some(style) = nested(&self.themes, parent).and_then(Value::as_table)
                {
                    for (key, value) in style {
                        document.set(&[parent[0], parent[1], key], Some(value.clone()))?;
                    }
                }
                // A present style with no explicit colors means inheritance, not a missing preset.
                if document
                    .value(&[parent[0], parent[1], "modifiers"])
                    .is_none()
                {
                    document.set(
                        &[parent[0], parent[1], "modifiers"],
                        Some(Value::String(String::new())),
                    )?;
                }
            }
            document.set(&field.keys(), value)?;
            let themes = Value::try_from(Styles::deserialize(&document.text())?)?;
            self.raw_themes = toml::from_str(&document.text())?;
            self.themes_document = Some(document);
            self.themes = themes;
        } else {
            let mut document = self.settings_document.clone();
            let value = match (&field.kind, value) {
                (Kind::TitleMode, Some(Value::String(mode))) => match mode.as_str() {
                    "None" => None,
                    "Date" => Some(Value::Table(toml::map::Map::from_iter([(
                        "kind".into(),
                        Value::String("date".into()),
                    )]))),
                    "Literal" => Some(
                        document
                            .value(&["auto_title"])
                            .filter(Value::is_str)
                            .unwrap_or(Value::String(String::new())),
                    ),
                    _ => bail!("Unknown title mode."),
                },
                (_, value) => value,
            };
            document.set(&field.keys(), value)?;
            let settings = settings_value(&toml::from_str::<Settings>(&document.text())?)?;
            self.raw_settings = toml::from_str(&document.text())?;
            self.settings_document = document;
            self.settings = settings;
        }
        self.status = "Change staged. Ctrl-S saves this section.".into();
        self.error = false;
        Ok(())
    }

    pub fn render_widget(&mut self, frame: &mut Frame, area: Rect, styles: &Styles) {
        let area = centered_rect(90, 80, area);
        frame.render_widget(Clear, area);
        let title = if self.current_tab() == Tab::Appearance {
            self.themes_document
                .as_ref()
                .map(|document| format!("Configuration · {}", safe_text(document.label())))
        } else {
            None
        }
        .unwrap_or_else(|| "Configuration".into());
        let block = Block::default()
            .title(format!(
                "{title}{}",
                if self.is_dirty() { " *" } else { "" }
            ))
            .borders(Borders::ALL);
        let inner = block.inner(area).inner(Margin::new(1, 0));
        frame.render_widget(block, area);
        if inner.width < 18 || inner.height < 8 {
            let message = if self.confirm_close || self.confirm_theme_close {
                "Discard changes? y/n"
            } else {
                "Enlarge terminal. Esc closes."
            };
            frame.render_widget(Paragraph::new(message).wrap(Wrap { trim: false }), inner);
            return;
        }
        if self.current_tab() == Tab::Appearance && self.themes_document.is_none() {
            self.render_theme_list(frame, inner, styles);
            return;
        }
        if self.current_tab() == Tab::Context {
            self.render_context(frame, inner);
            return;
        }
        let compact = inner.height < 14;
        let tabs_height = if compact { 1 } else { 3 };
        let destination_height = u16::from(!compact);
        let editor_height = if self.editor.is_some() { 3 } else { 0 };
        let status_height = if compact { 1 } else { 2 };
        let footer_height = if compact { 1 } else { 2 };
        let details_height = inner
            .height
            .saturating_sub(
                tabs_height
                    + destination_height
                    + editor_height
                    + status_height
                    + footer_height
                    + 2,
            )
            .min(if inner.height >= 22 { 6 } else { 3 });
        let chunks = Layout::vertical([
            Constraint::Length(tabs_height),
            Constraint::Length(destination_height),
            Constraint::Min(2),
            Constraint::Length(details_height),
            Constraint::Length(editor_height),
            Constraint::Length(status_height),
            Constraint::Length(footer_height),
        ])
        .split(inner);
        self.render_tabs(frame, chunks[0]);
        let destination = match self.current_tab() {
            Tab::Appearance => self
                .themes_document
                .as_ref()
                .map(|document| format!("{} · {}", document.label(), document.path().display()))
                .unwrap_or_default(),
            Tab::Layout => "App state (saved separately from configuration and themes)".into(),
            _ => format!("Settings file: {}", self.settings_document.path().display()),
        };
        frame.render_widget(Paragraph::new(safe_text(&destination)), chunks[1]);
        let rows: Vec<_> = self
            .tab_fields()
            .map(|field| {
                let value = self.value(field);
                let source = if field.tab == Tab::Layout {
                    "state"
                } else if self.raw_value(field).is_some() {
                    "file"
                } else {
                    "default"
                };
                let differs = value != self.active_value(field);
                let status = if differs && field.restart {
                    format!("{source} · restart")
                } else if differs {
                    format!("{source} · changed")
                } else {
                    source.into()
                };
                Row::new(vec![
                    Cell::from(field.label.clone()),
                    Cell::from(display_value(&field.kind, value.as_ref())),
                    Cell::from(status),
                ])
            })
            .collect();
        self.table_state.select(Some(self.selected[self.tab]));
        self.page_size = chunks[2].height.saturating_sub(1).max(1) as usize;
        frame.render_stateful_widget(
            Table::new(
                rows,
                [
                    Constraint::Percentage(47),
                    Constraint::Percentage(30),
                    Constraint::Percentage(23),
                ],
            )
            .header(
                Row::new(["Setting", "Draft", "Source / status"])
                    .style(Style::default().fg(Color::LightBlue)),
            )
            .row_highlight_style(Style::default().add_modifier(Modifier::REVERSED)),
            chunks[2],
            &mut self.table_state,
        );
        let field = self.current_field();
        let mut details = format!(
            "{}\n{}: {}  |  Default: {}\n{}",
            field.label,
            if field.tab == Tab::Appearance {
                "Saved"
            } else {
                "Active"
            },
            display_value(&field.kind, self.active_value(field).as_ref()),
            display_value(&field.kind, self.default_value(field).as_ref()),
            field.hint
        );
        if field.restart {
            details.push_str("\nThis session keeps its active value until restart.");
        }
        frame.render_widget(
            Paragraph::new(safe_text(&details)).wrap(Wrap { trim: false }),
            chunks[3],
        );
        if let Some(editor) = &mut self.editor {
            editor.set_block(
                Block::default()
                    .title("Edit value · Enter accepts · Esc cancels")
                    .borders(Borders::ALL),
            );
            editor.set_cursor_line_style(Style::default());
            editor.set_style(Style::from(if self.error {
                styles.general.input_block_invalid
            } else {
                styles.general.input_block_active
            }));
            editor.set_cursor_style(Style::from(styles.general.input_cursor_active));
            frame.render_widget(&*editor, chunks[4]);
        }
        let status = if self.confirm_theme_close && (compact || inner.width < 30) {
            "Discard? y/n"
        } else if self.confirm_theme_close {
            "Discard theme edits? y/n"
        } else if self.confirm_close && (compact || inner.width < 60) {
            "Discard? y/n"
        } else if self.confirm_close {
            "Discard all unsaved settings, appearance and layout changes? y = discard / n = continue"
        } else {
            &self.status
        };
        frame.render_widget(
            Paragraph::new(safe_text(status))
                .style(Style::default().fg(if self.error {
                    styles.msgbox.error
                } else {
                    styles.msgbox.info
                }))
                .wrap(Wrap { trim: false }),
            chunks[5],
        );
        let footer = if self.current_tab() == Tab::Appearance && self.editor.is_none() {
            if compact && inner.width < 30 {
                "^S Save Esc Back"
            } else if compact || inner.width < 60 {
                "Enter Edit  ^S Save  Esc Back"
            } else {
                "↑↓/PgUp/PgDn: fields  Tab/←→: sections  Enter/Space: edit\nd: use default  Ctrl-S: save theme  Esc: theme list  q: close"
            }
        } else if compact && self.editor.is_some() {
            "Save ^S Cancel Esc"
        } else if compact && inner.width < 30 {
            "Save ^S Close Esc"
        } else if compact {
            "Tab: next  ^S: save  Esc: close"
        } else if inner.width < 60 && self.editor.is_some() {
            "Enter: accept  Ctrl-S: save\nEsc: cancel"
        } else if inner.width < 60 {
            "Tab: sections  Enter: edit\nCtrl-S: save  Esc: close"
        } else if self.editor.is_some() {
            "Enter: accept field  Ctrl-S: accept + save  Esc: cancel field"
        } else {
            "↑↓/PgUp/PgDn: fields  Tab/←→: sections  Enter/Space: edit\nd: use default  Ctrl-S: save current document  Esc/q: close"
        };
        frame.render_widget(Paragraph::new(footer).wrap(Wrap { trim: false }), chunks[6]);
    }

    fn render_context(&mut self, frame: &mut Frame, area: Rect) {
        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(area);
        self.render_tabs(frame, chunks[0]);
        let mut content = vec![
            format!("Settings file: {}", self.settings_document.path().display()),
            format!("Themes directory: {}", self.theme_store.directory().display()),
            String::new(),
            "The Draft column shows the file's next-launch value, including defaults. Active shows the current session's value. CLI overrides are not written into these documents.".into(),
            "Restart-required storage, state-directory and history changes keep their active values for this session.".into(),
            "Ctrl-S saves one document: General/Entries/Editor/Storage share config.toml; Appearance lists themes; edit a theme and Ctrl-S saves it; Layout saves app state separately.".into(),
            "Theme 'inherit' leaves a color unspecified; Reset uses the terminal's default color; d restores the property's built-in preset.".into(),
            String::new(),
            "Launch context:".into(),
        ];
        content.extend(self.launch_details.clone());
        if self.launch_details.is_empty() {
            content.push("No additional command-line overrides were supplied.".into());
        }
        let lines: Vec<Line> = content
            .iter()
            .flat_map(|line| {
                textwrap::wrap(&safe_text(line), chunks[1].width.max(1) as usize)
                    .into_iter()
                    .map(|line| Line::from(line.into_owned()))
                    .collect::<Vec<_>>()
            })
            .collect();
        self.page_size = chunks[1].height.max(1) as usize;
        self.context_scroll = self
            .context_scroll
            .min(lines.len().saturating_sub(self.page_size));
        let visible: Vec<Line> = lines
            .into_iter()
            .skip(self.context_scroll)
            .take(self.page_size)
            .collect();
        frame.render_widget(Paragraph::new(visible), chunks[1]);
        let footer = if self.confirm_close && area.width < 60 {
            "Discard? y/n"
        } else if self.confirm_close {
            "Discard all unsaved changes? y = discard / n = continue"
        } else if area.width < 30 {
            "↑↓ Scroll Tab Next\nEsc Close"
        } else if area.width < 60 {
            "↑↓: scroll  Tab: sections\nEsc: close"
        } else {
            "↑↓/PgUp/PgDn/Home/End: scroll  Tab/←→: sections  Esc/q: close"
        };
        frame.render_widget(Paragraph::new(footer).wrap(Wrap { trim: false }), chunks[2]);
    }
}

fn validate_path(field: &Field, value: Option<&Value>) -> Result<()> {
    let Some(Value::String(path)) = value else {
        return Ok(());
    };
    let directory = match field.keys().as_slice() {
        ["json_backend" | "sqlite_backend", "file_path"] => false,
        ["vjournal_backend" | "file_backend", "directory"] | ["app_state_dir"] => true,
        _ => return Ok(()),
    };
    match std::fs::metadata(path) {
        Ok(metadata) => ensure!(
            if directory {
                metadata.is_dir()
            } else {
                metadata.is_file()
            },
            "Existing path must be a {}.",
            if directory {
                "directory"
            } else {
                "regular file"
            }
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    Ok(())
}

fn settings_value(settings: &Settings) -> Result<Value> {
    let mut settings = settings.clone();
    settings.complete_missing_options()?;
    Ok(Value::try_from(settings)?)
}

fn nested<'a>(value: &'a Value, path: &[&str]) -> Option<&'a Value> {
    path.iter().try_fold(value, |value, key| value.get(*key))
}

fn field_value(field: &Field, values: &Value) -> Option<Value> {
    let value = nested(values, &field.keys());
    match field.kind {
        Kind::TitleMode => Some(Value::String(
            match value {
                Some(Value::String(_)) => "Literal",
                Some(Value::Table(_)) => "Date",
                _ => "None",
            }
            .into(),
        )),
        Kind::TitleLiteral => value.filter(|value| value.is_str()).cloned(),
        _ => value.cloned(),
    }
}

fn editable_value(kind: &Kind, value: Option<&Value>) -> String {
    match (kind, value) {
        (Kind::TagPalette, Some(value)) => palette_text(value),
        (_, Some(Value::String(value))) => value.clone(),
        (_, Some(value)) => value.to_string(),
        (_, None) => String::new(),
    }
}

fn display_value(kind: &Kind, value: Option<&Value>) -> String {
    match value {
        Some(Value::String(value)) if value.is_empty() => "(empty)".into(),
        Some(Value::Array(values)) if values.is_empty() => "(none)".into(),
        None => "(unset / inherit)".into(),
        _ => safe_text(&editable_value(kind, value)),
    }
}

/// Formats a tag palette as the `FG on BG` pairs its field accepts.
fn palette_text(value: &Value) -> String {
    match value.clone().try_into::<Vec<TagColors>>() {
        Ok(palette) => palette
            .iter()
            .map(|colors| format!("{} on {}", colors.foreground, colors.background))
            .collect::<Vec<_>>()
            .join(", "),
        Err(_) => value.to_string(),
    }
}

fn parse_tag_colors(pair: &str) -> Result<TagColors> {
    let invalid = || {
        anyhow::anyhow!(
            "{pair:?} is not an FG on BG pair. Separate pairs with commas, e.g. Black on LightCyan, White on #3A5F8F."
        )
    };
    let words: Vec<_> = pair.split_whitespace().collect();
    let on = words
        .iter()
        .position(|word| word.eq_ignore_ascii_case("on"))
        .ok_or_else(invalid)?;
    let foreground = words[..on].join(" ").parse().map_err(|_| invalid())?;
    let background = words[on + 1..].join(" ").parse().map_err(|_| invalid())?;
    Ok(TagColors::new(foreground, background))
}

fn safe_text(value: &str) -> String {
    value
        .chars()
        .flat_map(|ch| {
            if ch.is_control() && ch != '\n' {
                ch.escape_default().collect::<Vec<_>>()
            } else {
                vec![ch]
            }
        })
        .collect()
}

fn parse_input(kind: &Kind, input: &str) -> Result<Option<Value>> {
    ensure!(
        !input.chars().any(char::is_control),
        "Enter a single-line value without control characters."
    );
    match kind {
        Kind::Number { min, max } => {
            let value: u64 = input
                .trim()
                .parse()
                .map_err(|_| anyhow::anyhow!("Enter a whole number from {min} to {max}."))?;
            ensure!(
                (*min..=*max).contains(&value),
                "Value must be from {min} to {max}."
            );
            Ok(Some(Value::Integer(value.try_into()?)))
        }
        Kind::Divider => {
            let value: i64 = input
                .trim()
                .parse()
                .map_err(|_| anyhow::anyhow!("Enter a whole-number percentage from 10 to 60."))?;
            ensure!(
                (10..=60).contains(&value),
                "Pane width must be 10-60 percent."
            );
            Ok(Some(Value::Integer(value)))
        }
        Kind::Text { allow_empty } => {
            ensure!(
                *allow_empty || !input.trim().is_empty(),
                "Enter a value, or cancel and press d to use the default."
            );
            Ok(Some(Value::String(input.into())))
        }
        Kind::TitleLiteral => Ok(Some(Value::String(input.into()))),
        Kind::Extension => {
            ensure!(
                !input.contains(['/', '\\']),
                "Filename extensions cannot contain directory separators."
            );
            Ok(Some(Value::String(input.into())))
        }
        Kind::Color { optional } => {
            if *optional && input.trim().eq_ignore_ascii_case("inherit") {
                return Ok(None);
            }
            let color: Color = input.trim().parse().map_err(|_| {
                anyhow::anyhow!(
                    "Use a color name, #RRGGBB, palette index 0-255, or Reset{}.",
                    if *optional { ", or inherit" } else { "" }
                )
            })?;
            Ok(Some(Value::try_from(color)?))
        }
        Kind::Modifiers => {
            let modifiers: Modifier = Value::String(input.trim().to_uppercase()).try_into().map_err(|_| anyhow::anyhow!("Use modifier names separated by | (e.g. BOLD | UNDERLINED), or leave empty."))?;
            Ok(Some(Value::try_from(modifiers)?))
        }
        Kind::TagPalette => {
            let palette = input
                .split(',')
                .map(str::trim)
                .filter(|pair| !pair.is_empty())
                .map(parse_tag_colors)
                .collect::<Result<Vec<_>>>()?;
            Ok(Some(Value::try_from(palette)?))
        }
        _ => bail!("This setting uses a toggle or choice."),
    }
}

#[cfg(test)]
mod tests;

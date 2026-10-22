use backend::DataProvider;
use crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::Rect;

use crate::app::{
    App, HandleInputReturnType,
    state::{MAX_ENTRIES_LIST_PERCENTAGE, MIN_ENTRIES_LIST_PERCENTAGE},
};

use super::UIComponents;

const MIN_PANE_WIDTH: u16 = 6;
pub(super) const MIN_SPLIT_WIDTH: u16 = MIN_PANE_WIDTH * 2;

#[derive(Debug, Clone, Copy)]
pub(super) struct PaneSplit {
    pub area: Rect,
    pub list: Rect,
    pub editor: Rect,
}

pub(super) struct DividerDrag {
    offset: u16,
    column: u16,
}

impl PaneSplit {
    pub fn new(area: Rect, percentage: u16) -> Option<Self> {
        if area.width < MIN_SPLIT_WIDTH || area.height < 3 {
            return None;
        }
        let preferred_width = (u32::from(area.width) * u32::from(percentage) / 100) as u16;
        let list_width = preferred_width.clamp(MIN_PANE_WIDTH, area.width - MIN_PANE_WIDTH);
        Some(Self {
            area,
            list: Rect::new(area.x, area.y, list_width, area.height),
            editor: Rect::new(
                area.x + list_width,
                area.y,
                area.width - list_width,
                area.height,
            ),
        })
    }

    fn hit_offset(self, column: u16, row: u16) -> Option<u16> {
        if row < self.area.y || row >= self.area.bottom() {
            return None;
        }
        if column == self.list.right() - 1 || column == self.editor.x {
            Some(self.list.right() - column)
        } else {
            None
        }
    }

    fn percentage_at(self, column: u16, offset: u16) -> u16 {
        let width = column
            .saturating_sub(self.area.x)
            .saturating_add(offset)
            .min(self.area.width);
        let area_width = u32::from(self.area.width);
        let width = u32::from(width);
        let lower = width * 100 / area_width;
        let upper = (width * 100).div_ceil(area_width);
        let lower_width = area_width * lower / 100;
        let upper_width = area_width * upper / 100;
        if width.abs_diff(lower_width) < width.abs_diff(upper_width) {
            lower as u16
        } else {
            upper as u16
        }
    }
}

impl UIComponents<'_> {
    pub(super) fn adjust_divider<D: DataProvider>(
        &mut self,
        app: &mut App<D>,
        delta: i16,
    ) -> HandleInputReturnType {
        if app.state.full_screen || self.has_popup() {
            return HandleInputReturnType::Ignore;
        }
        let cancelled_drag = self.divider_drag.take().is_some();
        let percentage = app
            .state
            .entries_list_percentage()
            .saturating_add_signed(delta);
        if app.state.set_entries_list_percentage(percentage) || cancelled_drag {
            HandleInputReturnType::Handled
        } else {
            HandleInputReturnType::Ignore
        }
    }

    pub fn cancel_divider_drag(&mut self) {
        self.divider_drag = None;
    }

    fn apply_divider_percentage<D: DataProvider>(&self, app: &mut App<D>, percentage: u16) -> bool {
        let percentage = percentage.clamp(MIN_ENTRIES_LIST_PERCENTAGE, MAX_ENTRIES_LIST_PERCENTAGE);
        let width_changes = self.pane_split.is_some_and(|split| {
            PaneSplit::new(split.area, percentage)
                .is_some_and(|proposed| proposed.list.width != split.list.width)
        });
        width_changes && app.state.set_entries_list_percentage(percentage)
    }

    pub fn handle_mouse_input<D: DataProvider>(
        &mut self,
        mouse: MouseEvent,
        app: &mut App<D>,
    ) -> HandleInputReturnType {
        if app.state.full_screen || self.has_popup() {
            self.cancel_divider_drag();
            return HandleInputReturnType::Ignore;
        }
        let Some(split) = self.pane_split else {
            return HandleInputReturnType::Ignore;
        };
        match mouse.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let was_dragging = self.divider_drag.is_some();
                self.divider_drag =
                    split
                        .hit_offset(mouse.column, mouse.row)
                        .map(|offset| DividerDrag {
                            offset,
                            column: mouse.column,
                        });
                if self.divider_drag.is_some() || was_dragging {
                    HandleInputReturnType::Handled
                } else {
                    HandleInputReturnType::Ignore
                }
            }
            MouseEventKind::Drag(MouseButton::Left) => {
                let Some(drag) = self.divider_drag.as_mut() else {
                    return HandleInputReturnType::Ignore;
                };
                if mouse.column == drag.column {
                    return HandleInputReturnType::Ignore;
                }
                drag.column = mouse.column;
                let percentage = split.percentage_at(mouse.column, drag.offset);
                if self.apply_divider_percentage(app, percentage) {
                    HandleInputReturnType::Handled
                } else {
                    HandleInputReturnType::Ignore
                }
            }
            MouseEventKind::Up(MouseButton::Left) => {
                if self.divider_drag.take().is_some() {
                    HandleInputReturnType::Handled
                } else {
                    HandleInputReturnType::Ignore
                }
            }
            _ => HandleInputReturnType::Ignore,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_fits_small_and_offset_rectangles() {
        for width in 0..150 {
            for percentage in [10, 30, 60] {
                let area = Rect::new(3, 4, width, 10);
                if let Some(split) = PaneSplit::new(area, percentage) {
                    assert!(split.list.width >= MIN_PANE_WIDTH);
                    assert!(split.editor.width >= MIN_PANE_WIDTH);
                    assert_eq!(split.list.right(), split.editor.x);
                    assert_eq!(split.editor.right(), area.right());
                } else {
                    assert!(width < MIN_SPLIT_WIDTH);
                }
            }
        }
    }

    #[test]
    fn only_shared_border_is_draggable() {
        let split = PaneSplit::new(Rect::new(5, 2, 100, 20), 30).unwrap();
        assert_eq!(split.hit_offset(34, 3), Some(1));
        assert_eq!(split.hit_offset(35, 3), Some(0));
        assert_eq!(split.hit_offset(33, 3), None);
        assert_eq!(split.hit_offset(36, 3), None);
        assert_eq!(split.hit_offset(34, 1), None);
        assert_eq!(split.hit_offset(34, 22), None);
        assert_eq!(split.percentage_at(34, 1), 30);
        assert_eq!(split.percentage_at(35, 0), 30);
        assert_eq!(split.percentage_at(u16::MAX, 1), 100);
    }

    #[test]
    fn border_position_round_trips_to_the_same_rendered_width() {
        for width in MIN_SPLIT_WIDTH..512 {
            for percentage in MIN_ENTRIES_LIST_PERCENTAGE..=MAX_ENTRIES_LIST_PERCENTAGE {
                let split = PaneSplit::new(Rect::new(5, 2, width, 20), percentage).unwrap();
                for (column, offset) in [(split.list.right() - 1, 1), (split.editor.x, 0)] {
                    let restored = PaneSplit::new(
                        split.area,
                        split
                            .percentage_at(column, offset)
                            .clamp(MIN_ENTRIES_LIST_PERCENTAGE, MAX_ENTRIES_LIST_PERCENTAGE),
                    )
                    .unwrap();
                    assert_eq!(restored.list.width, split.list.width);
                }
            }
        }
    }

    #[cfg(feature = "json")]
    mod input {
        use super::*;
        use backend::JsonDataProvide;
        use crossterm::event::{KeyCode, KeyModifiers};
        use ratatui::{Terminal, backend::TestBackend};

        use crate::{
            app::{
                keymap::Input,
                ui::{ControlType, Styles, editor::EditorMode},
            },
            settings::Settings,
        };

        async fn setup() -> (
            tempfile::TempDir,
            App<JsonDataProvide>,
            UIComponents<'static>,
        ) {
            let dir = tempfile::tempdir().unwrap();
            let provider = JsonDataProvide::new(dir.path().join("entries.json"));
            let mut app = App::new(provider, Settings::default());
            let id = app
                .add_entry("Journal".into(), chrono::Utc::now(), vec![], None)
                .await
                .unwrap();
            let mut ui = UIComponents::new(Styles::default());
            ui.set_current_entry(Some(id), &mut app);
            ui.change_active_control(ControlType::EntryContentTxt);
            ui.editor.set_editor_mode(EditorMode::Insert);
            ui.editor
                .set_entry_content("Unfinished draft\nwith another line", &app);
            (dir, app, ui)
        }

        fn mouse(kind: MouseEventKind, column: u16, row: u16) -> MouseEvent {
            MouseEvent {
                kind,
                column,
                row,
                modifiers: KeyModifiers::NONE,
            }
        }

        #[tokio::test]
        async fn keyboard_resize_preserves_draft_focus_and_editor_mode() {
            let (_dir, mut app, mut ui) = setup().await;
            let mut terminal = Terminal::new(TestBackend::new(100, 25)).unwrap();
            terminal.draw(|frame| ui.render_ui(frame, &app)).unwrap();
            let id = app.current_entry_id;
            let draft = ui.editor.get_content();
            for mode in [EditorMode::Normal, EditorMode::Insert, EditorMode::Visual] {
                ui.editor.set_editor_mode(mode);
                ui.handle_input(&Input::new(KeyCode::Char('.'), KeyModifiers::ALT), &mut app)
                    .await
                    .unwrap();
                assert_eq!(app.state.entries_list_percentage(), 35);
                terminal.draw(|frame| ui.render_ui(frame, &app)).unwrap();
                ui.handle_input(&Input::new(KeyCode::Char(','), KeyModifiers::ALT), &mut app)
                    .await
                    .unwrap();
                assert_eq!(app.state.entries_list_percentage(), 30);
                terminal.draw(|frame| ui.render_ui(frame, &app)).unwrap();
                assert_eq!(ui.editor.get_editor_mode(), mode);
                assert_eq!(ui.editor.get_content(), draft);
                assert!(ui.has_unsaved());
                assert_eq!(app.current_entry_id, id);
                assert_eq!(ui.active_control, ControlType::EntryContentTxt);
                assert!(app.get_current_entry().unwrap().content.is_empty());
            }
        }

        #[tokio::test]
        async fn only_divider_drag_changes_size_and_preserves_draft() {
            let (_dir, mut app, mut ui) = setup().await;
            let mut terminal = Terminal::new(TestBackend::new(100, 25)).unwrap();
            terminal.draw(|frame| ui.render_ui(frame, &app)).unwrap();
            let split = ui.pane_split.unwrap();
            let row = split.area.y + 1;
            let id = app.current_entry_id;
            let draft = ui.editor.get_content();

            for event in [
                mouse(MouseEventKind::Down(MouseButton::Left), 5, row),
                mouse(MouseEventKind::Drag(MouseButton::Left), 49, row),
                mouse(MouseEventKind::ScrollDown, 5, row),
            ] {
                assert_eq!(
                    ui.handle_mouse_input(event, &mut app),
                    HandleInputReturnType::Ignore
                );
            }
            assert_eq!(app.state.entries_list_percentage(), 30);

            ui.handle_mouse_input(
                mouse(
                    MouseEventKind::Down(MouseButton::Left),
                    split.list.right() - 1,
                    row,
                ),
                &mut app,
            );
            ui.handle_mouse_input(
                mouse(MouseEventKind::Drag(MouseButton::Left), 49, row),
                &mut app,
            );
            ui.handle_mouse_input(
                mouse(MouseEventKind::Up(MouseButton::Left), 49, row),
                &mut app,
            );
            assert_eq!(app.state.entries_list_percentage(), 50);
            assert!(ui.divider_drag.is_none());
            assert_eq!(app.current_entry_id, id);
            assert_eq!(ui.editor.get_content(), draft);
            assert!(ui.has_unsaved());
            assert_eq!(ui.editor.get_editor_mode(), EditorMode::Insert);
        }

        #[tokio::test]
        async fn vertical_only_drag_keeps_saved_percentage() {
            let (_dir, mut app, mut ui) = setup().await;
            for width in [12, 79] {
                let mut terminal = Terminal::new(TestBackend::new(width, 25)).unwrap();
                terminal.draw(|frame| ui.render_ui(frame, &app)).unwrap();
                let split = ui.pane_split.unwrap();
                for column in [split.list.right() - 1, split.editor.x] {
                    ui.handle_mouse_input(
                        mouse(MouseEventKind::Down(MouseButton::Left), column, 1),
                        &mut app,
                    );
                    assert_eq!(
                        ui.handle_mouse_input(
                            mouse(MouseEventKind::Drag(MouseButton::Left), column, 2),
                            &mut app,
                        ),
                        HandleInputReturnType::Ignore
                    );
                    assert_eq!(app.state.entries_list_percentage(), 30);
                    ui.handle_mouse_input(
                        mouse(MouseEventKind::Up(MouseButton::Left), column, 2),
                        &mut app,
                    );
                }
            }
        }

        #[tokio::test]
        async fn dragging_away_and_back_restores_original_divider_position() {
            let (_dir, mut app, mut ui) = setup().await;
            let mut terminal = Terminal::new(TestBackend::new(79, 25)).unwrap();
            terminal.draw(|frame| ui.render_ui(frame, &app)).unwrap();
            let initial = ui.pane_split.unwrap();
            let column = initial.list.right() - 1;
            ui.handle_mouse_input(
                mouse(MouseEventKind::Down(MouseButton::Left), column, 1),
                &mut app,
            );
            for column in [40, column] {
                assert_eq!(
                    ui.handle_mouse_input(
                        mouse(MouseEventKind::Drag(MouseButton::Left), column, 1),
                        &mut app,
                    ),
                    HandleInputReturnType::Handled
                );
                terminal.draw(|frame| ui.render_ui(frame, &app)).unwrap();
            }
            assert_eq!(app.state.entries_list_percentage(), 30);
            assert_eq!(ui.pane_split.unwrap().list.width, initial.list.width);
        }

        #[tokio::test]
        async fn drag_without_visible_width_change_keeps_saved_percentage() {
            let (_dir, mut app, mut ui) = setup().await;
            let mut terminal = Terminal::new(TestBackend::new(12, 25)).unwrap();
            terminal.draw(|frame| ui.render_ui(frame, &app)).unwrap();
            ui.handle_mouse_input(
                mouse(MouseEventKind::Down(MouseButton::Left), 5, 1),
                &mut app,
            );
            for column in [0, 11, 7, 4, 5] {
                assert_eq!(
                    ui.handle_mouse_input(
                        mouse(MouseEventKind::Drag(MouseButton::Left), column, 1),
                        &mut app,
                    ),
                    HandleInputReturnType::Ignore
                );
                assert_eq!(app.state.entries_list_percentage(), 30);
            }

            let mut terminal = Terminal::new(TestBackend::new(20, 25)).unwrap();
            terminal.draw(|frame| ui.render_ui(frame, &app)).unwrap();
            ui.handle_mouse_input(
                mouse(MouseEventKind::Down(MouseButton::Left), 5, 1),
                &mut app,
            );
            assert_eq!(
                ui.handle_mouse_input(
                    mouse(MouseEventKind::Drag(MouseButton::Left), 4, 1),
                    &mut app,
                ),
                HandleInputReturnType::Ignore
            );
            assert_eq!(app.state.entries_list_percentage(), 30);
            assert_eq!(
                ui.handle_mouse_input(
                    mouse(MouseEventKind::Drag(MouseButton::Left), 6, 1),
                    &mut app,
                ),
                HandleInputReturnType::Handled
            );
            assert_eq!(app.state.entries_list_percentage(), 35);
        }

        #[tokio::test]
        async fn cancelling_drag_requests_redraw_even_at_percentage_limit() {
            let (_dir, mut app, mut ui) = setup().await;
            let mut terminal = Terminal::new(TestBackend::new(100, 25)).unwrap();
            for (percentage, key) in [(10, ','), (60, '.')] {
                app.state.set_entries_list_percentage(percentage);
                terminal.draw(|frame| ui.render_ui(frame, &app)).unwrap();
                let column = ui.pane_split.unwrap().list.right() - 1;
                ui.handle_mouse_input(
                    mouse(MouseEventKind::Down(MouseButton::Left), column, 1),
                    &mut app,
                );
                assert_eq!(
                    ui.handle_input(&Input::new(KeyCode::Char(key), KeyModifiers::ALT), &mut app)
                        .await
                        .unwrap(),
                    HandleInputReturnType::Handled
                );
                assert!(ui.divider_drag.is_none());
                assert_eq!(app.state.entries_list_percentage(), percentage);

                ui.handle_mouse_input(
                    mouse(MouseEventKind::Down(MouseButton::Left), column, 1),
                    &mut app,
                );
                assert_eq!(
                    ui.handle_mouse_input(
                        mouse(MouseEventKind::Down(MouseButton::Left), 0, 1),
                        &mut app,
                    ),
                    HandleInputReturnType::Handled
                );
                assert!(ui.divider_drag.is_none());
                assert_eq!(app.state.entries_list_percentage(), percentage);
            }
        }

        #[tokio::test]
        async fn keyboard_resize_can_step_past_a_minimum_pane_width() {
            let (_dir, mut app, mut ui) = setup().await;
            let mut terminal = Terminal::new(TestBackend::new(19, 25)).unwrap();
            terminal.draw(|frame| ui.render_ui(frame, &app)).unwrap();
            for (percentage, width) in [(35, 6), (40, 7)] {
                ui.handle_input(&Input::new(KeyCode::Char('.'), KeyModifiers::ALT), &mut app)
                    .await
                    .unwrap();
                terminal.draw(|frame| ui.render_ui(frame, &app)).unwrap();
                assert_eq!(app.state.entries_list_percentage(), percentage);
                assert_eq!(ui.pane_split.unwrap().list.width, width);
            }
        }

        #[tokio::test]
        async fn popup_and_fullscreen_block_resize() {
            let (_dir, mut app, mut ui) = setup().await;
            ui.pane_split = PaneSplit::new(Rect::new(0, 0, 100, 20), 30);
            for fullscreen in [false, true] {
                app.state.full_screen = fullscreen;
                if !fullscreen {
                    ui.show_err_msg("Popup".into());
                }
                ui.handle_input(&Input::new(KeyCode::Char('.'), KeyModifiers::ALT), &mut app)
                    .await
                    .unwrap();
                ui.handle_mouse_input(
                    mouse(MouseEventKind::Down(MouseButton::Left), 29, 2),
                    &mut app,
                );
                ui.handle_mouse_input(
                    mouse(MouseEventKind::Drag(MouseButton::Left), 49, 2),
                    &mut app,
                );
                assert_eq!(app.state.entries_list_percentage(), 30);
                assert!(ui.divider_drag.is_none());
                ui.popup_stack.clear();
            }
        }

        #[tokio::test]
        async fn tiny_terminal_render_keeps_draft_and_percentage() {
            let (_dir, app, mut ui) = setup().await;
            let draft = ui.editor.get_content();
            for width in [0, 1, 5, 11, 12, 20] {
                for height in [0, 1, 3, 4, 6] {
                    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                    terminal.draw(|frame| ui.render_ui(frame, &app)).unwrap();
                }
            }
            assert_eq!(ui.editor.get_content(), draft);
            assert!(ui.has_unsaved());
            assert_eq!(app.state.entries_list_percentage(), 30);
        }

        #[tokio::test]
        async fn resizing_with_each_popup_open_preserves_draft() {
            use crate::app::ui::{
                EntryPopup, ExportPopup, FilterPopup, FuzzFindPopup, HelpPopup, MsgBox,
                MsgBoxActions, MsgBoxType, Popup, SortPopup, help_popup::KeybindingsTabs,
            };

            let (_dir, mut app, mut ui) = setup().await;
            let draft = ui.editor.get_content();
            let mut tags = EntryPopup::new_entry(&app.settings);
            tags.handle_input(
                &Input::new(KeyCode::Char('t'), KeyModifiers::CONTROL),
                &mut app,
            )
            .await
            .unwrap();
            let popups = [
                Popup::Help(Box::new(HelpPopup::new(KeybindingsTabs::Global))),
                Popup::Help(Box::new(HelpPopup::new(KeybindingsTabs::Editor))),
                Popup::Help(Box::new(HelpPopup::new(KeybindingsTabs::MultiSelect))),
                Popup::Entry(Box::new(EntryPopup::new_entry(&app.settings))),
                Popup::Entry(Box::new(tags)),
                Popup::MsgBox(Box::new(MsgBox::new(
                    MsgBoxType::Info("Message".into()),
                    MsgBoxActions::Ok,
                ))),
                Popup::Export(Box::new(ExportPopup::create_multi_select(&app).unwrap())),
                Popup::Filter(Box::new(FilterPopup::new(vec![], None))),
                Popup::FuzzFind(Box::new(FuzzFindPopup::new(Default::default()))),
                Popup::Sort(Box::new(SortPopup::new(&app.state.sorter))),
            ];

            for popup in popups {
                ui.popup_stack.push(popup);
                for width in [0, 1, 11, 12, 20, 40, 80] {
                    for height in [0, 1, 3, 4, 6, 12, 24] {
                        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                        terminal.draw(|frame| ui.render_ui(frame, &app)).unwrap();
                        assert!(ui.pane_split.is_none());
                        assert_eq!(ui.popup_stack.len(), 1);
                    }
                }
                ui.popup_stack.clear();
            }
            assert_eq!(ui.editor.get_content(), draft);
            assert!(ui.has_unsaved());
            assert_eq!(app.state.entries_list_percentage(), 30);
        }
    }
}

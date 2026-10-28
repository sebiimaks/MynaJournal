use super::*;
use backend::{EntriesDTO, Entry, EntryDraft, ModifyEntryError};
use chrono::Utc;
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, style::Color};

struct UnusedProvider;

impl DataProvider for UnusedProvider {
    async fn load_all_entries(&mut self) -> anyhow::Result<Vec<Entry>> {
        unreachable!()
    }
    async fn add_entry(&mut self, _: EntryDraft) -> Result<Entry, ModifyEntryError> {
        unreachable!()
    }
    async fn restore_entry(&mut self, _: Entry) -> Result<Entry, ModifyEntryError> {
        unreachable!()
    }
    async fn remove_entry(&mut self, _: u32) -> anyhow::Result<()> {
        unreachable!()
    }
    async fn update_entry(&mut self, _: Entry) -> Result<Entry, ModifyEntryError> {
        unreachable!()
    }
    async fn get_export_object(&mut self, _: &[u32]) -> anyhow::Result<EntriesDTO> {
        unreachable!()
    }
    async fn assign_priority_to_entries(&mut self, _: u32) -> anyhow::Result<()> {
        unreachable!()
    }
}

fn fixture(content: &str, mode: EditorMode) -> (Editor<'static>, App<UnusedProvider>) {
    let settings = crate::settings::Settings {
        sync_os_clipboard: false,
        ..Default::default()
    };
    let mut app = App::new(UnusedProvider, settings);
    app.entries.push(Entry::new(
        1,
        Utc::now(),
        "Test".into(),
        content.into(),
        Vec::new(),
        None,
    ));
    app.current_entry_id = Some(1);
    let mut editor = Editor::new();
    editor.set_current_entry(Some(1), &app);
    editor.text_area.move_cursor(CursorMove::Jump(0, 0));
    editor.set_active(true);
    editor.set_editor_mode(mode);
    (editor, app)
}

fn input(editor: &mut Editor<'_>, app: &App<UnusedProvider>, key: KeyCode, mods: KeyModifiers) {
    let event = Input::new(key, mods);
    if editor.is_insert_mode() {
        editor.handle_input_prioritized(&event, app).unwrap();
    } else {
        editor.handle_input(&event, app).unwrap();
    }
}

fn render(editor: &mut Editor<'_>, width: u16, height: u16) -> Buffer {
    let mut styles = Styles::default();
    styles.editor.selection_style.bg = Some(Color::Magenta);
    styles.editor.cursor_normal.bg = Some(Color::Yellow);
    styles.editor.cursor_insert.bg = Some(Color::Yellow);
    styles.editor.cursor_visual.bg = Some(Color::Yellow);
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|frame| editor.render_widget(frame, frame.area(), &styles))
        .unwrap();
    terminal.backend().buffer().clone()
}

fn cursor_cells(buffer: &Buffer) -> Vec<(u16, u16)> {
    let mut positions = Vec::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            if buffer[(x, y)].bg == Color::Yellow {
                positions.push((x, y));
            }
        }
    }
    positions
}

#[test]
fn shift_selection_is_visible_and_plain_navigation_clears_it() {
    let (mut editor, app) = fixture("abcdefghij", EditorMode::Normal);
    editor.text_area.move_cursor(CursorMove::Jump(0, 1));
    render(&mut editor, 8, 6);
    input(&mut editor, &app, KeyCode::Down, KeyModifiers::SHIFT);
    assert_eq!(editor.text_area.selection_range(), Some(((0, 1), (0, 7))));
    let buffer = render(&mut editor, 8, 6);
    for x in 2..7 {
        assert_eq!(buffer[(x, 1)].bg, Color::Magenta);
    }
    assert_eq!(buffer[(1, 2)].bg, Color::Magenta);
    assert_eq!(buffer[(2, 2)].bg, Color::Yellow);
    input(&mut editor, &app, KeyCode::Right, KeyModifiers::NONE);
    assert_eq!(editor.text_area.selection_range(), None);
    assert_eq!(editor.get_content(), "abcdefghij");
    assert!(!editor.has_unsaved());
}

#[test]
fn visual_selection_cut_and_undo_preserve_logical_lines() {
    let original = "abcdefghij\nnext";
    let (mut editor, app) = fixture(original, EditorMode::Normal);
    editor.text_area.move_cursor(CursorMove::Jump(0, 1));
    editor.set_editor_mode(EditorMode::Visual);
    render(&mut editor, 8, 6);
    input(&mut editor, &app, KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(editor.text_area.selection_range(), Some(((0, 1), (0, 7))));
    input(&mut editor, &app, KeyCode::Char('d'), KeyModifiers::NONE);
    assert_eq!(editor.get_content(), "ahij\nnext");
    input(&mut editor, &app, KeyCode::Char('u'), KeyModifiers::NONE);
    assert_eq!(editor.get_content(), original);
    assert!(!editor.has_unsaved());
}

#[test]
fn page_navigation_uses_visual_rows_in_insert_and_normal_modes() {
    let original = "abcdefghijklmnopqrstuvwxyz0123456789";
    for mode in [EditorMode::Insert, EditorMode::Normal] {
        let (mut editor, app) = fixture(original, mode);
        render(&mut editor, 10, 6);
        input(&mut editor, &app, KeyCode::PageDown, KeyModifiers::NONE);
        assert_eq!(editor.text_area.cursor(), (0, 24), "mode={mode:?}");
        assert!(!cursor_cells(&render(&mut editor, 10, 6)).is_empty());
        input(&mut editor, &app, KeyCode::PageUp, KeyModifiers::NONE);
        assert_eq!(editor.text_area.cursor(), (0, 0), "mode={mode:?}");
        assert_eq!(editor.get_content(), original);
        assert!(!editor.has_unsaved());
    }
}

#[test]
fn control_d_keeps_insert_deletion_and_normal_paging() {
    let original = "abcdefghijklmnopqrstuvwxyz";
    let (mut editor, app) = fixture(original, EditorMode::Insert);
    editor.text_area.move_cursor(CursorMove::Jump(0, 2));
    render(&mut editor, 8, 6);
    input(&mut editor, &app, KeyCode::Char('d'), KeyModifiers::CONTROL);
    assert_eq!(editor.get_content(), "abdefghijklmnopqrstuvwxyz");
    assert_eq!(editor.text_area.cursor(), (0, 2));
    editor.set_editor_mode(EditorMode::Normal);
    let content = editor.get_content();
    render(&mut editor, 8, 6);
    input(&mut editor, &app, KeyCode::Char('d'), KeyModifiers::CONTROL);
    assert!(editor.text_area.cursor().1 > 2);
    assert_eq!(editor.get_content(), content);
}

#[test]
fn redraw_resize_and_mode_changes_preserve_cursor_source_and_undo() {
    let original = "ab  cd\t界e\u{301}👩\u{200d}💻\r\n\n";
    let (mut editor, app) = fixture(original, EditorMode::Insert);
    render(&mut editor, 12, 6);
    input(&mut editor, &app, KeyCode::Char('Z'), KeyModifiers::NONE);
    let cursor = editor.text_area.cursor();
    for mode in [EditorMode::Normal, EditorMode::Insert, EditorMode::Normal] {
        editor.set_editor_mode(mode);
        for width in [7, 24, 10] {
            render(&mut editor, width, 6);
            assert_eq!(editor.text_area.cursor(), cursor);
            assert_eq!(editor.get_content(), format!("Z{original}"));
            assert!(editor.has_unsaved());
        }
    }
    input(&mut editor, &app, KeyCode::Char('u'), KeyModifiers::NONE);
    assert_eq!(editor.get_content(), original);
    assert!(!editor.has_unsaved());
    input(&mut editor, &app, KeyCode::Char('r'), KeyModifiers::CONTROL);
    assert_eq!(editor.get_content(), format!("Z{original}"));
}

#[test]
fn loading_and_external_content_preserve_trailing_newlines() {
    for original in ["", "a\n", "a\n\n", "a\r\nb\r\n"] {
        let (mut editor, app) = fixture(original, EditorMode::Normal);
        assert_eq!(editor.get_content(), original);
        render(&mut editor, 8, 6);
        assert_eq!(editor.get_content(), original);
        assert!(!editor.has_unsaved());
        editor.set_entry_content(original, &app);
        assert_eq!(editor.get_content(), original);
        assert!(!editor.has_unsaved());
    }
}

#[test]
fn crlf_line_endings_render_without_replacement_markers() {
    let original = "abc\r\nlast\r\n";
    let (mut editor, _) = fixture(original, EditorMode::Insert);
    let buffer = render(&mut editor, 12, 6);
    assert!(buffer.content.iter().all(|cell| cell.symbol() != "�"));
    assert_eq!(buffer[(4, 1)].symbol(), " ");
    assert_eq!(editor.get_content(), original);
}

#[test]
fn unicode_and_exact_width_end_cursors_remain_visible_when_resized() {
    for original in ["abcd", "界界", "e\u{301}👩\u{200d}💻", "\t界\n", "\r"] {
        let (mut editor, _) = fixture(original, EditorMode::Insert);
        for width in [3, 4, 6, 12, 3] {
            let count = editor.text_area.lines()[0].chars().count();
            for col in 0..=count {
                editor
                    .text_area
                    .move_cursor(CursorMove::Jump(0, col as u16));
                let buffer = render(&mut editor, width, 4);
                let cursor = cursor_cells(&buffer);
                assert!(
                    !cursor.is_empty(),
                    "body={original:?}, width={width}, col={col}"
                );
                assert!(
                    cursor
                        .iter()
                        .all(|&(x, y)| x > 0 && x < width - 1 && y > 0 && y < 3)
                );
                assert_eq!(editor.get_content(), original);
            }
        }
    }
}

#[test]
fn zero_sized_content_areas_do_not_change_source_or_cursor() {
    let original = "界👩\u{200d}💻\n";
    let (mut editor, _) = fixture(original, EditorMode::Insert);
    for (width, height) in [(0, 0), (1, 1), (2, 2), (2, 8), (8, 2)] {
        render(&mut editor, width, height);
        assert_eq!(editor.get_content(), original);
        assert_eq!(editor.text_area.cursor(), (0, 0));
    }
}

#[test]
fn deletion_at_a_wrap_boundary_uses_source_character_positions() {
    let original = "abcde\u{301}👩\u{200d}💻z";
    for key in [KeyCode::Delete, KeyCode::Backspace] {
        let (mut editor, app) = fixture(original, EditorMode::Insert);
        editor.text_area.move_cursor(CursorMove::Jump(0, 5));
        let mut reference = editor.text_area.clone();
        reference.input(KeyEvent::new(key, KeyModifiers::NONE));
        render(&mut editor, 7, 5);
        input(&mut editor, &app, key, KeyModifiers::NONE);
        assert_eq!(editor.text_area.lines(), reference.lines());
        assert_eq!(editor.text_area.cursor(), reference.cursor());
        assert!(!cursor_cells(&render(&mut editor, 7, 5)).is_empty());
    }
}

#[test]
fn modified_arrows_keep_paragraph_and_document_navigation() {
    let (mut editor, app) = fixture("abcdefghijklmnop\n\nlast", EditorMode::Insert);
    render(&mut editor, 8, 6);
    input(&mut editor, &app, KeyCode::Down, KeyModifiers::CONTROL);
    assert_eq!(editor.text_area.cursor(), (2, 0));
    input(
        &mut editor,
        &app,
        KeyCode::Up,
        KeyModifiers::CONTROL | KeyModifiers::ALT,
    );
    assert_eq!(editor.text_area.cursor(), (0, 0));
    assert!(!editor.has_unsaved());
}

#[test]
fn vertical_navigation_retains_column_across_short_rows() {
    let (mut editor, app) = fixture("abcdef\nx\nabcdef", EditorMode::Insert);
    editor.text_area.move_cursor(CursorMove::Jump(0, 5));
    render(&mut editor, 10, 6);
    input(&mut editor, &app, KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(editor.text_area.cursor(), (1, 1));
    input(&mut editor, &app, KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(editor.text_area.cursor(), (2, 5));
}

#[test]
fn reversing_after_a_top_edge_snap_uses_the_line_start_column() {
    for mode in [EditorMode::Insert, EditorMode::Normal] {
        let (mut editor, app) = fixture("abcdef\nabcdef", mode);
        editor.text_area.move_cursor(CursorMove::Jump(0, 5));
        render(&mut editor, 10, 6);
        input(&mut editor, &app, KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(editor.text_area.cursor(), (0, 0), "mode={mode:?}");
        input(&mut editor, &app, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(editor.text_area.cursor(), (1, 0), "mode={mode:?}");
        assert!(!editor.has_unsaved());
    }
}

#[test]
fn reversing_after_a_bottom_edge_snap_uses_the_line_end_column() {
    for mode in [EditorMode::Insert, EditorMode::Normal] {
        let (mut editor, app) = fixture("abcdef\nabcdef", mode);
        editor.text_area.move_cursor(CursorMove::Jump(1, 0));
        render(&mut editor, 10, 6);
        input(&mut editor, &app, KeyCode::Down, KeyModifiers::NONE);
        assert_eq!(editor.text_area.cursor(), (1, 6), "mode={mode:?}");
        input(&mut editor, &app, KeyCode::Up, KeyModifiers::NONE);
        assert_eq!(editor.text_area.cursor(), (0, 6), "mode={mode:?}");
        assert!(!editor.has_unsaved());
    }
}

#[test]
fn normal_end_motion_resets_the_vertical_target_column() {
    let (mut editor, app) = fixture("abcdef\nabcdef\nabcdef", EditorMode::Normal);
    editor.text_area.move_cursor(CursorMove::Jump(0, 2));
    render(&mut editor, 10, 6);
    input(&mut editor, &app, KeyCode::Down, KeyModifiers::NONE);
    input(&mut editor, &app, KeyCode::End, KeyModifiers::NONE);
    assert_eq!(editor.text_area.cursor(), (1, 6));
    input(&mut editor, &app, KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(editor.text_area.cursor(), (2, 6));
}

#[test]
fn resizing_preserves_selection_and_keeps_its_cursor_visible() {
    let original = "abcdefghijklmno\n界界界";
    let (mut editor, app) = fixture(original, EditorMode::Insert);
    editor.text_area.move_cursor(CursorMove::Jump(0, 2));
    render(&mut editor, 8, 6);
    input(&mut editor, &app, KeyCode::PageDown, KeyModifiers::SHIFT);
    let selection = editor.text_area.selection_range();
    let cursor = editor.text_area.cursor();
    assert!(selection.is_some());
    for width in [5, 20, 8] {
        let buffer = render(&mut editor, width, 5);
        assert_eq!(editor.text_area.selection_range(), selection);
        assert_eq!(editor.text_area.cursor(), cursor);
        assert!(!cursor_cells(&buffer).is_empty());
        assert_eq!(editor.get_content(), original);
        assert!(!editor.has_unsaved());
    }
}

#[test]
fn raw_opt_out_keeps_logical_line_navigation_and_undo_history() {
    let original = "abcdefghijklmnop\nsecond\n";
    let (mut editor, app) = fixture(original, EditorMode::Insert);
    render(&mut editor, 8, 6);
    input(&mut editor, &app, KeyCode::Char('Z'), KeyModifiers::NONE);
    editor.set_word_wrap(false);
    render(&mut editor, 8, 6);
    input(&mut editor, &app, KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(editor.text_area.cursor(), (1, 1));
    let cursor = editor.text_area.cursor();
    editor.set_word_wrap(true);
    render(&mut editor, 8, 6);
    assert_eq!(editor.text_area.cursor(), cursor);
    editor.set_editor_mode(EditorMode::Normal);
    input(&mut editor, &app, KeyCode::Char('u'), KeyModifiers::NONE);
    assert_eq!(editor.get_content(), original);
    assert!(!editor.has_unsaved());
}

#[test]
fn navigation_near_the_end_of_a_large_paragraph_does_not_truncate_columns() {
    let original = "a".repeat(70_000);
    let (mut editor, app) = fixture(&original, EditorMode::Insert);
    editor.text_area.move_cursor(CursorMove::End);
    render(&mut editor, 82, 8);
    input(&mut editor, &app, KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(editor.text_area.cursor(), (0, 69_920));
    input(&mut editor, &app, KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(editor.text_area.cursor(), (0, 70_000));
    assert!(!cursor_cells(&render(&mut editor, 82, 8)).is_empty());
    assert_eq!(editor.get_content().len(), original.len());
    assert!(!editor.has_unsaved());
}

fn render_ui(ui: &mut crate::app::UIComponents<'_>, app: &App<UnusedProvider>) {
    let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
    terminal.draw(|frame| ui.render_ui(frame, app)).unwrap();
}

#[tokio::test]
async fn ui_dispatch_resize_preserves_draft_selection_and_insert_undo() {
    let original = format!("{}\r\n\n", "alpha beta ".repeat(200));
    let (editor, mut app) = fixture(&original, EditorMode::Insert);
    let mut ui = crate::app::UIComponents::new(Styles::default());
    ui.editor = editor;
    ui.change_active_control(crate::app::ui::ControlType::EntryContentTxt);
    render_ui(&mut ui, &app);
    for (key, modifiers) in [
        (KeyCode::Char('X'), KeyModifiers::NONE),
        (KeyCode::PageDown, KeyModifiers::NONE),
        (KeyCode::Down, KeyModifiers::SHIFT),
    ] {
        ui.handle_input(&Input::new(key, modifiers), &mut app)
            .await
            .unwrap();
        render_ui(&mut ui, &app);
    }
    assert_eq!(ui.editor.get_content(), format!("X{original}"));
    let cursor = ui.editor.text_area.cursor();
    assert_eq!(cursor.0, 0);
    assert!(cursor.1 > 1);
    let selection = ui.editor.text_area.selection_range();
    assert!(selection.is_some());
    for (key, percentage) in [('.', 35), (',', 30)] {
        ui.handle_input(&Input::new(KeyCode::Char(key), KeyModifiers::ALT), &mut app)
            .await
            .unwrap();
        render_ui(&mut ui, &app);
        assert_eq!(app.state.entries_list_percentage(), percentage);
        assert_eq!(ui.editor.text_area.cursor(), cursor);
        assert_eq!(ui.editor.text_area.selection_range(), selection);
        assert_eq!(ui.editor.get_content(), format!("X{original}"));
        assert_eq!(ui.editor.get_editor_mode(), EditorMode::Insert);
        assert_eq!(
            ui.active_control,
            crate::app::ui::ControlType::EntryContentTxt
        );
    }
    ui.handle_input(
        &Input::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
        &mut app,
    )
    .await
    .unwrap();
    assert_eq!(ui.editor.get_content(), original);
    assert!(!ui.has_unsaved());
    ui.handle_input(
        &Input::new(KeyCode::Char('r'), KeyModifiers::CONTROL),
        &mut app,
    )
    .await
    .unwrap();
    assert_eq!(ui.editor.get_content(), format!("X{original}"));
    assert!(ui.has_unsaved());
    assert_eq!(app.get_current_entry().unwrap().content, original);
}

#[tokio::test]
async fn ui_dispatch_keeps_word_navigation_and_fullscreen_shortcuts() {
    let original = "alpha beta gamma delta ".repeat(10);
    let (editor, mut app) = fixture(&original, EditorMode::Insert);
    let mut ui = crate::app::UIComponents::new(Styles::default());
    ui.editor = editor;
    ui.change_active_control(crate::app::ui::ControlType::EntryContentTxt);
    ui.editor.text_area.move_cursor(CursorMove::Jump(0, 10));
    render_ui(&mut ui, &app);
    ui.handle_input(&Input::new(KeyCode::Left, KeyModifiers::CONTROL), &mut app)
        .await
        .unwrap();
    assert_eq!(ui.editor.text_area.cursor(), (0, 6));
    ui.handle_input(&Input::new(KeyCode::Esc, KeyModifiers::NONE), &mut app)
        .await
        .unwrap();
    assert_eq!(ui.editor.get_editor_mode(), EditorMode::Normal);
    let cursor = ui.editor.text_area.cursor();
    ui.handle_input(
        &Input::new(KeyCode::Char('f'), KeyModifiers::CONTROL),
        &mut app,
    )
    .await
    .unwrap();
    render_ui(&mut ui, &app);
    assert!(app.state.full_screen);
    assert_eq!(ui.editor.text_area.cursor(), cursor);
    assert_eq!(ui.editor.get_content(), original);
    assert!(!ui.has_unsaved());
}

#[test]
fn raw_crlf_rendering_preserves_visible_cursor_source_and_undo() {
    let original = "alpha beta\r\nlast\r\n";
    let (mut editor, app) = fixture(original, EditorMode::Insert);
    editor.set_word_wrap(false);
    for width in [16, 5, 12] {
        for col in [0, 10, 11] {
            editor.text_area.move_cursor(CursorMove::Jump(0, col));
            let buffer = render(&mut editor, width, 6);
            assert!(buffer.content.iter().all(|cell| {
                !cell.symbol().chars().any(char::is_control) && cell.symbol() != "�"
            }));
            assert!(
                !cursor_cells(&buffer).is_empty(),
                "width={width}, col={col}"
            );
            assert_eq!(editor.text_area.cursor(), (0, usize::from(col)));
            assert_eq!(editor.get_content(), original);
            assert!(!editor.has_unsaved());
        }
    }
    editor.text_area.move_cursor(CursorMove::Jump(0, 0));
    input(&mut editor, &app, KeyCode::Char('Z'), KeyModifiers::NONE);
    render(&mut editor, 5, 6);
    assert_eq!(editor.get_content(), format!("Z{original}"));
    input(&mut editor, &app, KeyCode::Char('u'), KeyModifiers::CONTROL);
    assert_eq!(editor.get_content(), original);
    assert!(!editor.has_unsaved());
    input(&mut editor, &app, KeyCode::Char('r'), KeyModifiers::CONTROL);
    assert_eq!(editor.get_content(), format!("Z{original}"));
}

#[test]
fn raw_control_characters_render_as_safe_symbols_without_changing_source() {
    let original = "a\u{1b}b\u{7}c\td";
    let (mut editor, _) = fixture(original, EditorMode::Insert);
    editor.set_word_wrap(false);
    let buffer = render(&mut editor, 24, 6);
    assert!(
        buffer
            .content
            .iter()
            .all(|cell| !cell.symbol().chars().any(char::is_control))
    );
    assert!(buffer.content.iter().any(|cell| cell.symbol() == "�"));
    assert_eq!(editor.get_content(), original);
    assert!(!editor.has_unsaved());
}

#[test]
fn raw_controls_keep_unicode_selection_and_cursor_visible_while_scrolling() {
    let original = "α\u{1b}中e\u{301}\tword\r\nplain text\r\nlast";
    let (mut editor, _) = fixture(original, EditorMode::Visual);
    editor.set_word_wrap(false);
    editor.text_area.move_cursor(CursorMove::Jump(0, 0));
    editor.text_area.start_selection();
    for col in 1..=11 {
        editor.text_area.move_cursor(CursorMove::Jump(0, col));
        for width in [6, 12, 24] {
            let buffer = render(&mut editor, width, 4);
            assert!(
                !cursor_cells(&buffer).is_empty(),
                "width={width}, col={col}"
            );
            assert!(
                buffer
                    .content
                    .iter()
                    .all(|cell| !cell.symbol().chars().any(char::is_control))
            );
            assert_eq!(
                editor.text_area.selection_range(),
                Some(((0, 0), (0, usize::from(col))))
            );
            assert_eq!(editor.get_content(), original);
            assert!(!editor.has_unsaved());
        }
    }
    editor.text_area.move_cursor(CursorMove::Jump(1, 5));
    let buffer = render(&mut editor, 12, 4);
    assert!(!cursor_cells(&buffer).is_empty());
    assert!(buffer.content.iter().any(|cell| cell.bg == Color::Magenta));
    assert_eq!(editor.get_content(), original);
}

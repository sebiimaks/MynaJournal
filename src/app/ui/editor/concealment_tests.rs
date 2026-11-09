use super::*;
use crate::app::test::create_default_app;
use backend::Entry;
use chrono::Utc;
use ratatui::{Terminal, backend::TestBackend, buffer::Buffer, style::Color};

fn fixture(content: &str, wrap: bool) -> (Editor<'static>, App<impl DataProvider>) {
    let mut app = create_default_app();
    app.settings.sync_os_clipboard = false;
    app.entries = vec![Entry::new(
        1,
        Utc::now(),
        String::new(),
        content.into(),
        vec![],
        None,
    )];
    app.current_entry_id = Some(1);
    let mut editor = Editor::new();
    editor.set_current_entry(Some(1), &app);
    editor.set_active(true);
    editor.set_editor_mode(EditorMode::Insert);
    editor.set_word_wrap(wrap);
    editor.set_text_mask(Some(TextMask::new()));
    (editor, app)
}

fn input<D: DataProvider>(editor: &mut Editor<'_>, app: &App<D>, code: KeyCode) {
    editor
        .handle_input_prioritized(&Input::new(code, KeyModifiers::NONE), app)
        .unwrap();
}

fn type_text<D: DataProvider>(editor: &mut Editor<'_>, app: &App<D>, text: &str) {
    for c in text.chars() {
        input(editor, app, KeyCode::Char(c));
    }
}

fn render(editor: &mut Editor<'_>, width: u16, height: u16) -> Buffer {
    let mut styles = Styles::default();
    styles.editor.cursor_insert.bg = Some(Color::Yellow);
    styles.editor.selection_style.bg = Some(Color::Magenta);
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal
        .draw(|f| editor.render_widget(f, f.area(), &styles, 0))
        .unwrap();
    terminal.backend().buffer().clone()
}

fn text(buffer: &Buffer) -> String {
    use unicode_width::UnicodeWidthStr;
    let mut result = String::new();
    for y in 0..buffer.area.height {
        let mut x = 0;
        while x < buffer.area.width {
            let symbol = buffer[(x, y)].symbol();
            result.push_str(symbol);
            x += symbol.width().max(1) as u16;
        }
    }
    result
}

#[test]
fn typing_reveals_only_the_active_word_and_delimiters_conceal_it() {
    for wrap in [false, true] {
        for delimiter in [
            KeyCode::Char(' '),
            KeyCode::Char('.'),
            KeyCode::Char(','),
            KeyCode::Char('!'),
            KeyCode::Char('/'),
            KeyCode::Char('。'),
            KeyCode::Enter,
            KeyCode::Tab,
        ] {
            let (mut editor, app) = fixture("privatehistory ", wrap);
            input(&mut editor, &app, KeyCode::End);
            type_text(&mut editor, &app, "don't-worry");
            let screen = text(&render(&mut editor, 80, 8));
            assert!(screen.contains("don't-worry"));
            assert!(!screen.contains("privatehistory"));
            input(&mut editor, &app, delimiter);
            assert!(editor.visible_word.is_none(), "{delimiter:?}");
            assert!(!text(&render(&mut editor, 80, 8)).contains("don't-worry"));
            assert!(
                editor
                    .get_content()
                    .starts_with("privatehistory don't-worry")
            );
        }
    }
}

#[test]
fn navigation_never_reveals_existing_words_and_typing_can_edit_one() {
    let (mut editor, app) = fixture("existingword otherword", true);
    input(&mut editor, &app, KeyCode::Home);
    for _ in 0..4 {
        input(&mut editor, &app, KeyCode::Right);
    }
    assert!(editor.visible_word.is_none());
    assert!(!text(&render(&mut editor, 60, 8)).contains("existingword"));
    type_text(&mut editor, &app, "X");
    assert!(text(&render(&mut editor, 60, 8)).contains("exisXtingword"));
    input(&mut editor, &app, KeyCode::Left);
    assert!(editor.visible_word.is_none());
    assert!(!text(&render(&mut editor, 60, 8)).contains("exisXtingword"));
    input(&mut editor, &app, KeyCode::Char('\''));
    assert!(editor.visible_word.is_none());
}

#[test]
fn deletion_can_correct_the_active_word_but_cannot_reveal_the_previous_word() {
    for wrap in [false, true] {
        let (mut editor, app) = fixture("previousword ", wrap);
        input(&mut editor, &app, KeyCode::End);
        type_text(&mut editor, &app, "typing");
        input(&mut editor, &app, KeyCode::Backspace);
        assert!(text(&render(&mut editor, 60, 8)).contains("typin"));
        for _ in 0..5 {
            input(&mut editor, &app, KeyCode::Backspace);
        }
        assert!(editor.visible_word.is_none());
        input(&mut editor, &app, KeyCode::Backspace);
        assert_eq!(editor.get_content(), "previousword");
        assert!(editor.visible_word.is_none());
        assert!(!text(&render(&mut editor, 60, 8)).contains("previousword"));
    }
}

#[test]
fn unicode_words_and_connectors_remain_visible_until_a_boundary() {
    for word in [
        "don’t",
        "well‐being",
        "well‑being",
        "e\u{301}clair",
        "家族",
        "क्षेत्र",
    ] {
        let (mut editor, app) = fixture("", true);
        type_text(&mut editor, &app, word);
        assert!(text(&render(&mut editor, 60, 8)).contains(word), "{word}");
        input(&mut editor, &app, KeyCode::Char('.'));
        assert!(editor.visible_word.is_none());
        assert!(!text(&render(&mut editor, 60, 8)).contains(word), "{word}");
        assert_eq!(editor.get_content(), format!("{word}."));
    }
}

#[test]
fn masks_preserve_cursor_selection_and_geometry_at_scroll_and_wrap_boundaries() {
    for source in [
        "privateword\t家族 e\u{301}! morewords and-more\n👨‍👩‍👧‍👦 secondline, emoji.\nthirdline",
        "plain words and punctuation; a-hyphen.\nsecond line\nthird line",
        "家族 widecharacters.\nnext 家族 word\nthird",
        "tabs\tsecrets\tmore\nsecond\trow\nthird",
        "controls\u{1b} secrets\r\nnext\u{7}line\nthird",
    ] {
        for wrap in [false, true] {
            for width in [3, 8, 19, 60] {
                let (mut editor, app) = fixture(source, wrap);
                editor.text_area.move_cursor(CursorMove::Jump(0, 0));
                render(&mut editor, width, 6);
                for (code, modifiers) in [
                    (KeyCode::End, KeyModifiers::NONE),
                    (KeyCode::Left, KeyModifiers::SHIFT),
                    (KeyCode::Down, KeyModifiers::SHIFT),
                    (KeyCode::Home, KeyModifiers::NONE),
                    (KeyCode::PageDown, KeyModifiers::NONE),
                ] {
                    editor
                        .handle_input_prioritized(&Input::new(code, modifiers), &app)
                        .unwrap();
                    if modifiers == KeyModifiers::SHIFT {
                        assert!(editor.text_area.selection_range().is_some());
                    }
                    let hidden = render(&mut editor, width, 6);
                    let mask = editor.text_mask;
                    editor.set_text_mask(None);
                    let visible = render(&mut editor, width, 6);
                    for (index, (a, b)) in hidden.content.iter().zip(&visible.content).enumerate() {
                        assert_eq!(
                            a.bg,
                            b.bg,
                            "source={source:?}, wrap={wrap}, width={width}, key={code:?}, cell={index}, hidden={:?}, visible={:?}",
                            a.symbol(),
                            b.symbol()
                        );
                        if !b
                            .symbol()
                            .chars()
                            .any(super::super::text_mask::is_word_character)
                        {
                            assert_eq!(
                                a.symbol(),
                                b.symbol(),
                                "source={source:?}, wrap={wrap}, width={width}, key={code:?}, cell={index}, cursor={:?}\nhidden={hidden:?}\nvisible={visible:?}",
                                editor.text_area.cursor()
                            );
                        }
                    }
                    editor.set_text_mask(mask);
                }
                assert_eq!(editor.get_content(), source);
                assert!(!editor.has_unsaved());
            }
        }
    }
}

#[test]
fn active_word_remains_readable_across_wrapped_rows_and_resizing() {
    let (mut editor, app) = fixture("", true);
    render(&mut editor, 10, 8);
    type_text(&mut editor, &app, "verylongword");
    for width in [10, 6, 30] {
        let buffer = render(&mut editor, width, 8);
        let content: String = (1..7)
            .map(|y| {
                let row: String = (1..width - 1).map(|x| buffer[(x, y)].symbol()).collect();
                row.trim_end().to_string()
            })
            .collect();
        assert_eq!(content, "verylongword");
    }
    input(&mut editor, &app, KeyCode::Char(' '));
    assert!(editor.visible_word.is_none());
    assert!(!text(&render(&mut editor, 30, 8)).contains("verylongword"));
    assert_eq!(editor.get_content(), "verylongword ");
}

#[test]
fn selected_tab_stays_highlighted_when_partially_scrolled_out_of_view() {
    let (mut editor, app) = fixture("\tabc", false);
    editor.text_area.set_tab_length(4);
    editor.text_area.move_cursor(CursorMove::Jump(0, 0));
    render(&mut editor, 5, 6);
    editor
        .handle_input_prioritized(&Input::new(KeyCode::Right, KeyModifiers::SHIFT), &app)
        .unwrap();
    assert_eq!(editor.text_area.cursor(), tui_textarea::DataCursor(0, 1));
    assert_eq!(editor.text_area.selection_range(), Some(((0, 0), (0, 1))));

    for mask in [editor.text_mask, None, editor.text_mask] {
        editor.set_text_mask(mask);
        let buffer = render(&mut editor, 5, 6);
        for x in [1, 2] {
            assert_eq!(buffer[(x, 1)].symbol(), " ");
            assert_eq!(buffer[(x, 1)].bg, Color::Magenta);
        }
        assert_eq!(buffer[(3, 1)].bg, Color::Yellow);
        if mask.is_none() {
            assert_eq!(buffer[(3, 1)].symbol(), "a");
        } else {
            assert_ne!(buffer[(3, 1)].symbol(), "a");
        }
        assert_eq!(editor.get_content(), "\tabc");
        assert!(!editor.has_unsaved());
    }
}

#[test]
fn unwrapped_active_word_stays_visible_after_a_long_combined_emoji_prefix() {
    let prefix = format!("{} ", "👨‍👩‍👧‍👦".repeat(10));
    let (mut editor, app) = fixture(&prefix, false);
    render(&mut editor, 22, 6);
    type_text(&mut editor, &app, "currentword");
    let hidden = render(&mut editor, 22, 6);
    assert!(text(&hidden).contains("currentword"));
    assert_eq!(hidden[(20, 1)].bg, Color::Yellow);
    assert_eq!(hidden, render(&mut editor, 22, 6));

    for width in [18, 30, 22] {
        let hidden = render(&mut editor, width, 6);
        assert!(text(&hidden).contains("currentword"));
        assert_eq!(hidden, render(&mut editor, width, 6));
    }
    let hidden = render(&mut editor, 22, 6);
    editor.set_text_mask(None);
    assert_eq!(hidden, render(&mut editor, 22, 6));
    for width in [18, 30, 22] {
        let visible = render(&mut editor, width, 6);
        assert!(text(&visible).contains("currentword"));
        assert_eq!(visible, render(&mut editor, width, 6));
    }
    assert_eq!(editor.get_content(), format!("{prefix}currentword"));
}

#[test]
fn completed_words_do_not_reappear_after_undo_paste_or_focus_changes() {
    let (mut editor, app) = fixture("", true);
    type_text(&mut editor, &app, "draftword");
    editor.set_active(false);
    editor.set_active(true);
    assert!(editor.visible_word.is_none());
    type_text(&mut editor, &app, "x");
    editor.set_editor_mode(EditorMode::Normal);
    editor.set_editor_mode(EditorMode::Insert);
    assert!(editor.visible_word.is_none());
    type_text(&mut editor, &app, "y");
    editor
        .handle_input_prioritized(&Input::new(KeyCode::Char('u'), KeyModifiers::CONTROL), &app)
        .unwrap();
    assert!(editor.visible_word.is_none());
    editor.text_area.set_yank_text("pastedprivateword");
    editor
        .handle_input_prioritized(&Input::new(KeyCode::Char('y'), KeyModifiers::CONTROL), &app)
        .unwrap();
    assert!(editor.visible_word.is_none());
    assert!(!text(&render(&mut editor, 80, 8)).contains("pastedprivateword"));
}

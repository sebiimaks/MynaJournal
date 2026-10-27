//! ratatui's [`CrosstermBackend`] with a fix for the Linux text console (`TERM=linux`).
//!
//! The console draws light foreground colours by turning on its bold intensity, so its colour
//! codes set the intensity too: a palette colour (`38;5;N`) turns it on for a light colour and
//! off for a normal one, while the default-foreground code (SGR 39) leaves it as it was.
//! `CrosstermBackend` only sends the styles that change from one cell to the next, and sends a
//! cell's modifiers before its colours, so the intensity a cell got depended on the cells
//! before it. `BOLD` text in a normal colour was bright only when the previous cell had the
//! same colour, and text in the `Reset` colour drawn right after light text came out bright
//! white instead of the console's normal grey.
//!
//! On the console, every style change starts from a full attribute reset (SGR 0) and sends the
//! cell's colours before its modifiers, so each cell's intensity follows from its own style.
//! Other terminals get `CrosstermBackend` unchanged.

use std::io::{self, Write};

use crossterm::{
    cursor::MoveTo,
    queue,
    style::{Attribute, Colors, Print, SetAttribute, SetColors},
};
use ratatui::{
    backend::{Backend, ClearType, CrosstermBackend, WindowSize},
    buffer::Cell,
    layout::{Position, Size},
    style::{Color, Modifier},
};

pub struct ConsoleBackend<W: Write> {
    inner: CrosstermBackend<W>,
    linux_console: bool,
}

impl<W: Write> ConsoleBackend<W> {
    pub fn new(writer: W, linux_console: bool) -> Self {
        Self {
            inner: CrosstermBackend::new(writer),
            linux_console,
        }
    }
}

/// Whether a `TERM` value names the Linux text console, such as `linux` or `linux-16color`.
pub fn is_linux_console(term: &str) -> bool {
    term.starts_with("linux")
}

/// The modifiers `CrosstermBackend` sends, in the order it adds them.
const MODIFIER_ATTRIBUTES: [(Modifier, Attribute); 8] = [
    (Modifier::REVERSED, Attribute::Reverse),
    (Modifier::BOLD, Attribute::Bold),
    (Modifier::ITALIC, Attribute::Italic),
    (Modifier::UNDERLINED, Attribute::Underlined),
    (Modifier::DIM, Attribute::Dim),
    (Modifier::CROSSED_OUT, Attribute::CrossedOut),
    (Modifier::SLOW_BLINK, Attribute::SlowBlink),
    (Modifier::RAPID_BLINK, Attribute::RapidBlink),
];

impl<W: Write> ConsoleBackend<W> {
    /// `CrosstermBackend::draw` with every style change sent from SGR 0, colours first. The
    /// console has no underline colour, so none is sent.
    fn draw_on_console<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        // Reapply the first style, including defaults, after external output or a failed draw.
        let mut style = None;
        let mut last_pos: Option<Position> = None;
        for (x, y, cell) in content {
            if !matches!(last_pos, Some(p) if x == p.x + 1 && y == p.y) {
                queue!(self.inner, MoveTo(x, y))?;
            }
            last_pos = Some(Position { x, y });
            if Some((cell.fg, cell.bg, cell.modifier)) != style {
                let colors = Colors {
                    foreground: (cell.fg != Color::Reset).then(|| cell.fg.into()),
                    background: (cell.bg != Color::Reset).then(|| cell.bg.into()),
                };
                queue!(
                    self.inner,
                    SetAttribute(Attribute::Reset),
                    SetColors(colors)
                )?;
                for (modifier, attribute) in MODIFIER_ATTRIBUTES {
                    if cell.modifier.contains(modifier) {
                        queue!(self.inner, SetAttribute(attribute))?;
                    }
                }
                style = Some((cell.fg, cell.bg, cell.modifier));
            }
            queue!(self.inner, Print(cell.symbol()))?;
        }
        queue!(self.inner, SetAttribute(Attribute::Reset))
    }
}

impl<W: Write> Backend for ConsoleBackend<W> {
    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        if self.linux_console {
            self.draw_on_console(content)
        } else {
            self.inner.draw(content)
        }
    }

    fn append_lines(&mut self, n: u16) -> io::Result<()> {
        self.inner.append_lines(n)
    }

    fn hide_cursor(&mut self) -> io::Result<()> {
        self.inner.hide_cursor()
    }

    fn show_cursor(&mut self) -> io::Result<()> {
        self.inner.show_cursor()
    }

    fn get_cursor_position(&mut self) -> io::Result<Position> {
        self.inner.get_cursor_position()
    }

    fn set_cursor_position<P: Into<Position>>(&mut self, position: P) -> io::Result<()> {
        self.inner.set_cursor_position(position)
    }

    fn clear(&mut self) -> io::Result<()> {
        self.clear_region(ClearType::All)
    }

    fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
        if self.linux_console {
            // Erased cells inherit the current attributes on the Linux console.
            queue!(self.inner, SetAttribute(Attribute::Reset))?;
        }
        self.inner.clear_region(clear_type)
    }

    fn size(&self) -> io::Result<Size> {
        self.inner.size()
    }

    fn window_size(&mut self) -> io::Result<WindowSize> {
        self.inner.window_size()
    }

    fn flush(&mut self) -> io::Result<()> {
        Backend::flush(&mut self.inner)
    }
}

#[cfg(test)]
mod tests {
    use ratatui::{buffer::Buffer, layout::Rect, style::Style};

    use super::*;

    fn cell(symbol: &'static str, style: Style) -> Cell {
        let mut cell = Cell::new(symbol);
        cell.set_style(style);
        cell
    }

    fn positioned(cells: &[Cell]) -> impl Iterator<Item = (u16, u16, &Cell)> {
        cells.iter().zip(0..).map(|(cell, x)| (x, 0, cell))
    }

    fn draw(cells: &[Cell], linux_console: bool) -> Vec<u8> {
        enable_test_colors();
        let mut output = Vec::new();
        ConsoleBackend::new(&mut output, linux_console)
            .draw(positioned(cells))
            .unwrap();
        output
    }

    fn draw_with_crossterm(cells: &[Cell]) -> Vec<u8> {
        enable_test_colors();
        let mut output = Vec::new();
        CrosstermBackend::new(&mut output)
            .draw(positioned(cells))
            .unwrap();
        output
    }

    fn enable_test_colors() {
        // CI and terminal wrappers may set NO_COLOR; these tests inspect colour sequences.
        crossterm::style::force_color_output(true);
    }

    #[test]
    fn console_clear_resets_inherited_styles_and_keeps_scrollback() {
        let mut output = Vec::new();
        ConsoleBackend::new(&mut output, true).clear().unwrap();

        assert_eq!(output, b"\x1b[0m\x1b[2J");
    }

    #[test]
    fn console_region_clear_resets_styles_and_other_terminals_are_unchanged() {
        for clear_type in [ClearType::All, ClearType::AfterCursor] {
            let mut standard = Vec::new();
            CrosstermBackend::new(&mut standard)
                .clear_region(clear_type)
                .unwrap();
            for linux_console in [false, true] {
                let mut output = Vec::new();
                ConsoleBackend::new(&mut output, linux_console)
                    .clear_region(clear_type)
                    .unwrap();
                let expected = if linux_console {
                    [b"\x1b[0m".as_slice(), &standard].concat()
                } else {
                    standard.clone()
                };
                assert_eq!(output, expected);
            }
        }
    }

    #[test]
    fn first_plain_cell_resets_inherited_terminal_style() {
        for inherited in [b"\x1b[1m", b"\x1b[2m"] {
            let mut output = inherited.to_vec();
            let cells = [cell("a", Style::new())];
            ConsoleBackend::new(&mut output, true)
                .draw(positioned(&cells))
                .unwrap();
            assert_eq!(
                output,
                [inherited.as_slice(), b"\x1b[1;1H\x1b[0ma\x1b[0m"].concat()
            );
        }
    }

    #[test]
    fn failed_draw_propagates_error_and_next_draw_restores_plain_style() {
        #[derive(Default)]
        struct FailAfterFirstCell {
            output: Vec<u8>,
            failed: bool,
        }

        impl Write for FailAfterFirstCell {
            fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
                if !self.failed && self.output.last() == Some(&b'A') {
                    self.failed = true;
                    return Err(io::Error::other("simulated write failure"));
                }
                self.output.extend_from_slice(bytes);
                Ok(bytes.len())
            }

            fn flush(&mut self) -> io::Result<()> {
                Ok(())
            }
        }

        enable_test_colors();
        let mut writer = FailAfterFirstCell::default();
        let mut backend = ConsoleBackend::new(&mut writer, true);
        let cells = [cell("A", Style::new().fg(Color::LightRed))];
        let error = backend.draw(positioned(&cells)).unwrap_err();
        assert_eq!(error.to_string(), "simulated write failure");
        let cells = [cell("b", Style::new())];
        backend.draw(positioned(&cells)).unwrap();
        assert_eq!(
            writer.output,
            b"\x1b[1;1H\x1b[0m\x1b[38;5;9mA\x1b[1;1H\x1b[0mb\x1b[0m"
        );
    }

    #[test]
    fn rgb_and_all_palette_colours_precede_modifiers_and_repeated_styles_are_reused() {
        let palette = (0..=255).map(|index| (Color::Indexed(index), format!("38;5;{index}")));
        let rgb = [
            (Color::Rgb(255, 0, 0), "38;2;255;0;0".to_owned()),
            (Color::Rgb(100, 100, 100), "38;2;100;100;100".to_owned()),
        ];
        for (color, color_code) in palette.chain(rgb) {
            for (modifier, modifier_codes) in [
                (Modifier::empty(), ""),
                (Modifier::BOLD, "\x1b[1m"),
                (Modifier::DIM, "\x1b[2m"),
                (Modifier::BOLD | Modifier::DIM, "\x1b[1m\x1b[2m"),
            ] {
                let cells = [
                    cell("a", Style::new().fg(color).add_modifier(modifier)),
                    cell("b", Style::new().fg(color).add_modifier(modifier)),
                    cell("c", Style::new().bg(Color::LightRed)),
                ];
                assert_eq!(
                    draw(&cells, true),
                    format!(
                        "\x1b[1;1H\x1b[0m\x1b[{color_code}m{modifier_codes}ab\x1b[0m\x1b[48;5;9mc\x1b[0m"
                    ).as_bytes(),
                    "{color:?} {modifier:?}"
                );
            }
        }
    }

    #[test]
    fn empty_and_consecutive_draws_restore_default_style() {
        enable_test_colors();
        let mut output = Vec::new();
        let mut backend = ConsoleBackend::new(&mut output, true);
        let cells = [cell(
            "A",
            Style::new()
                .fg(Color::LightRed)
                .bg(Color::Blue)
                .add_modifier(Modifier::DIM),
        )];
        backend.draw(positioned(&cells)).unwrap();
        backend.draw(std::iter::empty()).unwrap();
        let cells = [cell("b", Style::new())];
        backend.draw(positioned(&cells)).unwrap();
        assert_eq!(
            output,
            b"\x1b[1;1H\x1b[0m\x1b[38;5;9;48;5;4m\x1b[2mA\x1b[0m\x1b[0m\x1b[1;1H\x1b[0mb\x1b[0m"
        );
        assert_eq!(draw(&[], true), b"\x1b[0m");
    }

    #[test]
    fn default_foreground_after_light_colour_gets_a_full_reset() {
        let cells = [
            cell("A", Style::new().fg(Color::LightRed)),
            cell("b", Style::new()),
            cell("c", Style::new()),
        ];

        let output = draw(&cells, true);

        assert_eq!(output, b"\x1b[1;1H\x1b[0m\x1b[38;5;9mA\x1b[0mbc\x1b[0m");
    }

    #[test]
    fn background_is_reapplied_after_resetting_a_light_foreground_to_default() {
        let cells = [
            cell("A", Style::new().fg(Color::LightRed).bg(Color::Blue)),
            cell("b", Style::new().bg(Color::Blue)),
        ];

        let output = draw(&cells, true);

        assert_eq!(
            output,
            b"\x1b[1;1H\x1b[0m\x1b[38;5;9;48;5;4mA\x1b[0m\x1b[48;5;4mb\x1b[0m"
        );
    }

    #[test]
    fn bold_is_reapplied_when_foreground_returns_to_default() {
        let bold = Style::new().add_modifier(Modifier::BOLD);
        let cells = [
            cell("A", bold.fg(Color::Red)),
            cell("b", bold),
            cell("C", bold.fg(Color::LightRed)),
            cell("d", bold),
        ];

        let output = draw(&cells, true);

        assert_eq!(
            output,
            b"\x1b[1;1H\x1b[0m\x1b[38;5;1m\x1b[1mA\x1b[0m\x1b[1mb\x1b[0m\x1b[38;5;9m\x1b[1mC\x1b[0m\x1b[1md\x1b[0m"
        );
    }

    #[test]
    fn dim_is_reapplied_when_light_foreground_returns_to_default() {
        let dim = Style::new().add_modifier(Modifier::DIM);
        let cells = [cell("A", dim.fg(Color::LightRed)), cell("b", dim)];

        let output = draw(&cells, true);

        assert_eq!(
            output,
            b"\x1b[1;1H\x1b[0m\x1b[38;5;9m\x1b[2mA\x1b[0m\x1b[2mb\x1b[0m"
        );
    }

    #[test]
    fn bold_is_applied_after_colours_even_when_only_modifiers_change() {
        let bold = Style::new().add_modifier(Modifier::BOLD);
        let cells = [
            cell("a", Style::new().fg(Color::Red)),
            cell("B", bold.fg(Color::Red)),
            cell("C", bold.fg(Color::Green)),
            cell("D", bold.fg(Color::Red).bg(Color::Blue)),
        ];

        let output = draw(&cells, true);

        assert_eq!(
            output,
            b"\x1b[1;1H\x1b[0m\x1b[38;5;1ma\x1b[0m\x1b[38;5;1m\x1b[1mB\x1b[0m\x1b[38;5;2m\x1b[1mC\x1b[0m\x1b[38;5;1;48;5;4m\x1b[1mD\x1b[0m"
        );
    }

    #[test]
    fn dim_is_reused_for_matching_styles_and_reapplied_after_colour_changes() {
        let dim = Style::new().add_modifier(Modifier::DIM);
        let cells = [
            cell("a", dim.fg(Color::Red)),
            cell("b", dim.fg(Color::Red)),
            cell("c", dim.fg(Color::LightGreen)),
        ];

        let output = draw(&cells, true);

        assert_eq!(
            output,
            b"\x1b[1;1H\x1b[0m\x1b[38;5;1m\x1b[2mab\x1b[0m\x1b[38;5;10m\x1b[2mc\x1b[0m"
        );
    }

    #[test]
    fn light_colour_is_reapplied_after_removing_bold() {
        let cells = [
            cell(
                "A",
                Style::new()
                    .fg(Color::LightRed)
                    .add_modifier(Modifier::BOLD),
            ),
            cell("B", Style::new().fg(Color::LightRed)),
        ];

        let output = draw(&cells, true);

        assert_eq!(
            output,
            b"\x1b[1;1H\x1b[0m\x1b[38;5;9m\x1b[1mA\x1b[0m\x1b[38;5;9mB\x1b[0m"
        );
    }

    #[test]
    fn console_draws_the_same_text_at_the_same_positions_as_crossterm() {
        fn without_sgr(output: &[u8]) -> String {
            let text = std::str::from_utf8(output).unwrap();
            let mut kept = String::new();
            let mut rest = text;
            while let Some(start) = rest.find("\x1b[") {
                kept.push_str(&rest[..start]);
                let after = &rest[start..];
                let end = after.find(|c: char| c.is_ascii_alphabetic()).unwrap();
                if !after[..=end].ends_with('m') {
                    kept.push_str(&after[..=end]);
                }
                rest = &after[end + 1..];
            }
            kept.push_str(rest);
            kept
        }

        let cells = [
            cell("a", Style::new()),
            cell(
                "B",
                Style::new()
                    .fg(Color::LightRed)
                    .add_modifier(Modifier::BOLD),
            ),
            cell("c", Style::new().fg(Color::Green).bg(Color::Blue)),
            cell("d", Style::new()),
        ];
        let content = [
            (0, 0, &cells[0]),
            (1, 0, &cells[1]),
            (4, 0, &cells[2]),
            (0, 2, &cells[3]),
        ];

        enable_test_colors();
        let mut console = Vec::new();
        ConsoleBackend::new(&mut console, true)
            .draw(content.into_iter())
            .unwrap();
        let mut crossterm = Vec::new();
        CrosstermBackend::new(&mut crossterm)
            .draw(content.into_iter())
            .unwrap();

        assert_eq!(without_sgr(&console), without_sgr(&crossterm));
        assert!(without_sgr(&console).contains("\x1b[1;5H"));

        let empty = Buffer::empty(Rect::new(0, 0, 12, 2));
        let mut wide = empty.clone();
        wide.set_string(0, 0, "界e\u{301}a", Style::new().fg(Color::LightRed));
        let mut narrow = empty.clone();
        narrow.set_string(0, 0, "abe\u{301}", Style::new());
        for (previous, next) in [(&empty, &wide), (&wide, &narrow)] {
            let updates = previous.diff(next);
            let mut console = Vec::new();
            ConsoleBackend::new(&mut console, true)
                .draw(updates.iter().copied())
                .unwrap();
            let mut crossterm = Vec::new();
            CrosstermBackend::new(&mut crossterm)
                .draw(updates.iter().copied())
                .unwrap();
            assert_eq!(without_sgr(&console), without_sgr(&crossterm));
            assert!(without_sgr(&console).contains("e\u{301}"));
        }
    }

    #[test]
    fn other_terminals_get_unchanged_crossterm_output() {
        let bold = Style::new().add_modifier(Modifier::BOLD);
        let cells = [
            cell("A", Style::new().fg(Color::LightRed)),
            cell("b", Style::new()),
            cell("C", bold.fg(Color::Yellow)),
            cell("d", bold),
        ];

        assert_eq!(draw(&cells, false), draw_with_crossterm(&cells));
    }

    #[test]
    fn linux_console_is_recognised_from_term() {
        assert!(is_linux_console("linux"));
        assert!(is_linux_console("linux-16color"));
        assert!(!is_linux_console("xterm-256color"));
        assert!(!is_linux_console("screen.linux"));
        assert!(!is_linux_console("tmux-256color"));
        assert!(!is_linux_console(""));
    }
}

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
        // Every draw ends with SGR 0, so the next one starts from the default style.
        let mut style = (Color::Reset, Color::Reset, Modifier::empty());
        let mut last_pos: Option<Position> = None;
        for (x, y, cell) in content {
            if !matches!(last_pos, Some(p) if x == p.x + 1 && y == p.y) {
                queue!(self.inner, MoveTo(x, y))?;
            }
            last_pos = Some(Position { x, y });
            if (cell.fg, cell.bg, cell.modifier) != style {
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
                style = (cell.fg, cell.bg, cell.modifier);
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
        self.inner.clear()
    }

    fn clear_region(&mut self, clear_type: ClearType) -> io::Result<()> {
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
    use ratatui::style::{Modifier, Style};

    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Intensity {
        Normal,
        Bold,
        Half,
    }

    /// The intensity the Linux console gives each printed character, following `csi_m` in
    /// `drivers/tty/vt/vt.c` for the codes these tests produce: palette colours 0-7 set normal
    /// intensity, 8-15 set bold, and 39 leaves it as it was.
    fn console_intensities(output: &[u8]) -> Vec<(char, Intensity)> {
        let mut chars = std::str::from_utf8(output).unwrap().chars();
        let mut intensity = Intensity::Normal;
        let mut printed = Vec::new();
        while let Some(ch) = chars.next() {
            if ch != '\x1b' {
                printed.push((ch, intensity));
                continue;
            }
            assert_eq!(chars.next(), Some('['));
            let mut params = String::new();
            let final_byte = loop {
                match chars.next().unwrap() {
                    c @ ('0'..='9' | ';') => params.push(c),
                    c => break c,
                }
            };
            if final_byte != 'm' {
                continue;
            }
            let mut codes = params.split(';').map(|code| code.parse::<u8>().unwrap());
            while let Some(code) = codes.next() {
                match code {
                    0 | 22 => intensity = Intensity::Normal,
                    1 => intensity = Intensity::Bold,
                    2 => intensity = Intensity::Half,
                    38 | 48 => {
                        assert_eq!(codes.next(), Some(5));
                        let index = codes.next().unwrap();
                        if code == 38 {
                            intensity = if (8..16).contains(&index) {
                                Intensity::Bold
                            } else {
                                Intensity::Normal
                            };
                        }
                    }
                    _ => {}
                }
            }
        }
        printed
    }

    fn cell(symbol: &'static str, style: Style) -> Cell {
        let mut cell = Cell::new(symbol);
        cell.set_style(style);
        cell
    }

    fn positioned(cells: &[Cell]) -> impl Iterator<Item = (u16, u16, &Cell)> {
        cells.iter().zip(0..).map(|(cell, x)| (x, 0, cell))
    }

    fn draw(cells: &[Cell], linux_console: bool) -> Vec<u8> {
        let mut output = Vec::new();
        ConsoleBackend::new(&mut output, linux_console)
            .draw(positioned(cells))
            .unwrap();
        output
    }

    fn draw_with_crossterm(cells: &[Cell]) -> Vec<u8> {
        let mut output = Vec::new();
        CrosstermBackend::new(&mut output)
            .draw(positioned(cells))
            .unwrap();
        output
    }

    #[test]
    fn default_foreground_after_light_colour_is_drawn_at_normal_intensity() {
        let cells = [
            cell("A", Style::new().fg(Color::LightRed)),
            cell("b", Style::new()),
            cell("c", Style::new()),
        ];

        let output = draw(&cells, true);

        let text = String::from_utf8(output.clone()).unwrap();
        assert!(text[text.find('A').unwrap()..text.find('b').unwrap()].contains("\x1b[0m"));
        assert!(text.contains("bc"));
        assert_eq!(
            console_intensities(&output),
            [
                ('A', Intensity::Bold),
                ('b', Intensity::Normal),
                ('c', Intensity::Normal)
            ]
        );
    }

    #[test]
    fn default_foreground_with_background_after_light_colour_is_drawn_at_normal_intensity() {
        let cells = [
            cell("A", Style::new().fg(Color::LightRed).bg(Color::Blue)),
            cell("b", Style::new().bg(Color::Blue)),
        ];

        let output = draw(&cells, true);

        assert_eq!(
            console_intensities(&output),
            [('A', Intensity::Bold), ('b', Intensity::Normal)]
        );
    }

    #[test]
    fn bold_default_foreground_after_colour_stays_bold() {
        let bold = Style::new().add_modifier(Modifier::BOLD);
        let cells = [
            cell("A", bold.fg(Color::Red)),
            cell("b", bold),
            cell("C", bold.fg(Color::LightRed)),
            cell("d", bold),
        ];

        let output = draw(&cells, true);

        let text = String::from_utf8(output.clone()).unwrap();
        assert!(text[text.find('C').unwrap()..text.find('d').unwrap()].contains("\x1b[1m"));
        assert_eq!(console_intensities(&output)[1], ('b', Intensity::Bold));
        assert_eq!(console_intensities(&output)[3], ('d', Intensity::Bold));
    }

    #[test]
    fn dim_default_foreground_after_light_colour_stays_dim() {
        let dim = Style::new().add_modifier(Modifier::DIM);
        let cells = [cell("A", dim.fg(Color::LightRed)), cell("b", dim)];

        let output = draw(&cells, true);

        assert_eq!(console_intensities(&output)[1], ('b', Intensity::Half));
    }

    #[test]
    fn bold_normal_colour_is_bright_whether_or_not_the_colour_changed() {
        let bold = Style::new().add_modifier(Modifier::BOLD);
        let cells = [
            cell("a", Style::new().fg(Color::Red)),
            cell("B", bold.fg(Color::Red)),
            cell("C", bold.fg(Color::Green)),
            cell("D", bold.fg(Color::Red).bg(Color::Blue)),
        ];

        let output = draw(&cells, true);

        assert_eq!(
            console_intensities(&output),
            [
                ('a', Intensity::Normal),
                ('B', Intensity::Bold),
                ('C', Intensity::Bold),
                ('D', Intensity::Bold)
            ]
        );
    }

    #[test]
    fn dim_colour_is_half_bright_whether_or_not_the_colour_changed() {
        let dim = Style::new().add_modifier(Modifier::DIM);
        let cells = [
            cell("a", dim.fg(Color::Red)),
            cell("b", dim.fg(Color::Red)),
            cell("c", dim.fg(Color::LightGreen)),
        ];

        let output = draw(&cells, true);

        assert_eq!(
            console_intensities(&output),
            [
                ('a', Intensity::Half),
                ('b', Intensity::Half),
                ('c', Intensity::Half)
            ]
        );
    }

    #[test]
    fn light_colour_stays_bright_when_bold_is_removed() {
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
            console_intensities(&output),
            [('A', Intensity::Bold), ('B', Intensity::Bold)]
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

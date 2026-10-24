//! ratatui's [`CrosstermBackend`] with a fix for the Linux text console (`TERM=linux`).
//!
//! The console draws light foreground colours by turning on its bold intensity, and its
//! default-foreground code (SGR 39) restores the colour but leaves that intensity on.
//! `CrosstermBackend` only sends the styles that change from one cell to the next, so text in
//! the `Reset` colour drawn right after light text came out bright white instead of the
//! console's normal grey, and could change between frames.
//!
//! On the console, a draw is split before every cell whose foreground changes to `Reset`.
//! Each `CrosstermBackend::draw` call ends with a full attribute reset (SGR 0) and sends the
//! next cell's modifiers from scratch, so plain text starts at normal intensity while `BOLD`
//! and `DIM` cells still get their own codes. Other terminals get `CrosstermBackend` unchanged.

use std::io::{self, Write};
use std::iter::Peekable;

use ratatui::{
    backend::{Backend, ClearType, CrosstermBackend, WindowSize},
    buffer::Cell,
    layout::{Position, Size},
    style::Color,
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

/// Yields cells up to the next one whose foreground changes to `Reset`.
struct UntilDefaultForeground<'s, I: Iterator> {
    cells: &'s mut Peekable<I>,
    previous_fg: Option<Color>,
}

impl<'a, I> Iterator for UntilDefaultForeground<'_, I>
where
    I: Iterator<Item = (u16, u16, &'a Cell)>,
{
    type Item = (u16, u16, &'a Cell);

    fn next(&mut self) -> Option<Self::Item> {
        let &(_, _, cell) = self.cells.peek()?;
        if cell.fg == Color::Reset && self.previous_fg.is_some_and(|fg| fg != Color::Reset) {
            return None;
        }
        self.previous_fg = Some(cell.fg);
        self.cells.next()
    }
}

impl<W: Write> Backend for ConsoleBackend<W> {
    fn draw<'a, I>(&mut self, content: I) -> io::Result<()>
    where
        I: Iterator<Item = (u16, u16, &'a Cell)>,
    {
        if !self.linux_console {
            return self.inner.draw(content);
        }

        let mut cells = content.peekable();
        loop {
            self.inner.draw(UntilDefaultForeground {
                cells: &mut cells,
                previous_fg: None,
            })?;
            if cells.peek().is_none() {
                return Ok(());
            }
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
    fn draws_without_a_change_to_default_foreground_are_unchanged_on_the_console() {
        let cells = [
            cell("a", Style::new()),
            cell("B", Style::new().fg(Color::LightRed)),
            cell("c", Style::new().fg(Color::Green)),
        ];

        assert_eq!(draw(&cells, true), draw_with_crossterm(&cells));
        assert_eq!(draw(&[], true), draw_with_crossterm(&[]));
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

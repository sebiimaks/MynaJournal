use std::{
    io,
    sync::atomic::{AtomicBool, Ordering},
};

use crossterm::{
    cursor::{MoveTo, Show},
    event::{DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture},
    execute,
    style::{Attribute, SetAttribute},
    terminal::{
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode,
    },
};

static SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);
static BRACKETED_PASTE_ACTIVE: AtomicBool = AtomicBool::new(false);

pub struct TerminalSession;

pub struct BracketedPasteMode;

impl BracketedPasteMode {
    pub fn enter() -> io::Result<Self> {
        BRACKETED_PASTE_ACTIVE.store(true, Ordering::Relaxed);
        let mode = Self;
        execute!(io::stdout(), EnableBracketedPaste)?;
        Ok(mode)
    }
}

impl Drop for BracketedPasteMode {
    fn drop(&mut self) {
        disable_bracketed_paste();
    }
}

fn disable_bracketed_paste() {
    if BRACKETED_PASTE_ACTIVE.swap(false, Ordering::Relaxed) {
        let _ = execute!(io::stdout(), DisableBracketedPaste);
    }
}

/// Clear a fullscreen viewport and invalidate its buffers without asking the
/// terminal for the cursor position, which conflicts with Crossterm's event reader.
pub fn clear_fullscreen<B: ratatui::backend::Backend>(
    terminal: &mut ratatui::Terminal<B>,
) -> Result<(), B::Error> {
    terminal.resize(terminal.size()?.into())
}

impl TerminalSession {
    pub fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        SESSION_ACTIVE.store(true, Ordering::Relaxed);
        let session = Self;
        execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture)?;
        Ok(session)
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        restore_terminal();
    }
}

pub fn restore_terminal() {
    // The panic hook restores before printing diagnostics; Drop must not erase them again.
    if !SESSION_ACTIVE.swap(false, Ordering::Relaxed) {
        return;
    }

    disable_bracketed_paste();
    let mut stdout = io::stdout();
    let _ = execute!(stdout, DisableMouseCapture);
    if std::env::var("TERM").is_ok_and(|term| crate::console_backend::is_linux_console(&term)) {
        // Linux VTs have no alternate screen to restore. Keep scrollback, clear the display.
        let _ = execute!(
            stdout,
            SetAttribute(Attribute::Reset),
            Clear(ClearType::All),
            MoveTo(0, 0)
        );
    }
    let _ = execute!(stdout, LeaveAlternateScreen, Show);
    let _ = disable_raw_mode();
}

#[cfg(all(test, unix))]
mod tests {
    use std::{
        fs,
        process::{Command, Stdio},
    };

    use super::*;

    fn assert_cleanup(test_name: &str, exit: &str) {
        const PROBE: &str = "TUI_JOURNAL_TEST_CLEANUP_PROBE";
        if std::env::var_os(PROBE).is_some() {
            restore_terminal();
            crate::chain_panic_hook();
            let result = std::panic::catch_unwind(|| -> io::Result<()> {
                SESSION_ACTIVE.store(true, Ordering::Relaxed);
                let _session = TerminalSession;
                match exit {
                    "error" => Err(io::Error::other("test application error")),
                    "panic" => panic!("test panic diagnostic"),
                    _ => Ok(()),
                }
            });
            match exit {
                "error" => assert!(result.unwrap().is_err()),
                "panic" => assert!(result.is_err()),
                _ => assert!(result.unwrap().is_ok()),
            }
            restore_terminal();
            return;
        }

        for term in [
            Some("linux"),
            Some("linux-16color"),
            Some("xterm-256color"),
            None,
        ] {
            // Isolate the global panic hook, session state, TERM, and terminal output.
            let mut command = Command::new(std::env::current_exe().unwrap());
            command
                .args(["--exact", test_name, "--nocapture"])
                .env(PROBE, "1");
            if let Some(term) = term {
                command.env("TERM", term);
            } else {
                command.env_remove("TERM");
            }
            let capture = tempfile::NamedTempFile::new().unwrap();
            let status = command
                .stdout(Stdio::from(capture.as_file().try_clone().unwrap()))
                .stderr(Stdio::from(capture.as_file().try_clone().unwrap()))
                .status()
                .unwrap();
            let output = fs::read_to_string(capture.path()).unwrap();
            assert!(status.success(), "{output}");
            assert_eq!(output.matches("\x1b[?1006l").count(), 1, "{output:?}");
            assert_eq!(output.matches("\x1b[?1049l").count(), 1, "{output:?}");
            assert_eq!(output.matches("\x1b[?25h").count(), 1, "{output:?}");
            assert!(!output.contains("\x1b[3J"), "{output:?}");
            if term.is_some_and(crate::console_backend::is_linux_console) {
                assert_eq!(output.matches("\x1b[2J").count(), 1, "{output:?}");
                assert!(output.contains("\x1b[0m\x1b[2J\x1b[1;1H\x1b[?1049l\x1b[?25h"));
            } else {
                assert!(!output.contains("\x1b[2J"), "{output:?}");
                assert!(!output.contains("\x1b[1;1H"), "{output:?}");
            }
            if exit == "panic" {
                assert!(
                    output.find("\x1b[?25h").unwrap()
                        < output.find("test panic diagnostic").unwrap(),
                    "{output:?}"
                );
            }
        }
    }

    #[test]
    fn normal_exit_restores_display_once() {
        assert_cleanup(
            "terminal_session::tests::normal_exit_restores_display_once",
            "normal",
        );
    }

    #[test]
    fn error_exit_restores_display_once() {
        assert_cleanup(
            "terminal_session::tests::error_exit_restores_display_once",
            "error",
        );
    }

    #[test]
    fn panic_cleanup_is_not_repeated_during_unwinding() {
        assert_cleanup(
            "terminal_session::tests::panic_cleanup_is_not_repeated_during_unwinding",
            "panic",
        );
    }

    #[test]
    fn bracketed_paste_ends_before_journal_input_on_return_error_and_panic() {
        const PROBE: &str = "MYNA_JOURNAL_TEST_BRACKETED_PASTE_PROBE";
        const NEXT_INPUT: &str = "journal input resumes";
        if let Ok(exit) = std::env::var(PROBE) {
            crate::chain_panic_hook();
            SESSION_ACTIVE.store(true, Ordering::Relaxed);
            let _session = TerminalSession;
            let result = std::panic::catch_unwind(|| -> io::Result<()> {
                let _paste = BracketedPasteMode::enter()?;
                match exit.as_str() {
                    "error" => Err(io::Error::other("test selector error")),
                    "panic" => panic!("test selector panic"),
                    _ => Ok(()),
                }
            });
            match exit.as_str() {
                "error" => assert!(result.unwrap().is_err()),
                "panic" => assert!(result.is_err()),
                _ => assert!(result.unwrap().is_ok()),
            }
            println!("{NEXT_INPUT}");
            return;
        }

        for exit in ["normal", "error", "panic"] {
            for term in ["linux", "xterm-256color"] {
                let capture = tempfile::NamedTempFile::new().unwrap();
                let status = Command::new(std::env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "terminal_session::tests::bracketed_paste_ends_before_journal_input_on_return_error_and_panic",
                        "--nocapture",
                    ])
                    .env(PROBE, exit)
                    .env("TERM", term)
                    .stdout(Stdio::from(capture.as_file().try_clone().unwrap()))
                    .stderr(Stdio::from(capture.as_file().try_clone().unwrap()))
                    .status()
                    .unwrap();
                let output = fs::read_to_string(capture.path()).unwrap();
                assert!(status.success(), "{output}");
                assert_eq!(output.matches("\x1b[?2004h").count(), 1, "{output:?}");
                assert_eq!(output.matches("\x1b[?2004l").count(), 1, "{output:?}");
                assert!(
                    output.find("\x1b[?2004h").unwrap() < output.find("\x1b[?2004l").unwrap(),
                    "{output:?}"
                );
                assert!(
                    output.find("\x1b[?2004l").unwrap() < output.find(NEXT_INPUT).unwrap(),
                    "{output:?}"
                );
                if exit == "panic" {
                    assert!(
                        output.find("\x1b[?2004l").unwrap()
                            < output.find("test selector panic").unwrap(),
                        "{output:?}"
                    );
                }
            }
        }
    }
}

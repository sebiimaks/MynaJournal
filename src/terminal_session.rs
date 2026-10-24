use std::{
    io,
    sync::atomic::{AtomicBool, Ordering},
};

use crossterm::{
    cursor::{MoveTo, Show},
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    style::{Attribute, SetAttribute},
    terminal::{
        Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
        enable_raw_mode,
    },
};

static SESSION_ACTIVE: AtomicBool = AtomicBool::new(false);

pub struct TerminalSession;

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
}

use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail};

use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use scopeguard::defer;
use tokio::process::Command;

use crate::settings::Settings;

const ENV_EDITOR_OPTIONS: [&str; 2] = ["VISUAL", "EDITOR"];

pub struct EditorDraft {
    directory: tempfile::TempDir,
    path: PathBuf,
}

impl EditorDraft {
    pub fn new(
        content: &str,
        extension: &str,
        preferred_dir: Option<&Path>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            !extension.contains(['/', '\\']) && !extension.chars().any(char::is_control),
            "External editor extension cannot contain directory separators or control characters"
        );
        let mut builder = tempfile::Builder::new();
        builder.prefix("myna-journal-editor-");
        let directory = preferred_dir.map_or_else(
            || builder.tempdir(),
            |path| builder.tempdir_in(path).or_else(|_| builder.tempdir()),
        )?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))?;
        }
        let extension = extension.trim_start_matches('.');
        let name = if extension.is_empty() {
            "entry".to_owned()
        } else {
            format!("entry.{extension}")
        };
        let path = directory.path().join(name);
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(&path)?.write_all(content.as_bytes())?;
        Ok(Self { directory, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn read(self) -> anyhow::Result<String> {
        let read = || -> anyhow::Result<String> {
            anyhow::ensure!(
                fs::symlink_metadata(&self.path)?.is_file(),
                "The editor draft is no longer a regular file"
            );
            Ok(fs::read_to_string(&self.path)?)
        };
        match read() {
            Ok(content) => Ok(content),
            Err(error) => {
                // An editor may replace the file with less restrictive permissions.
                // Keeping its private parent protects it without chmod through a symlink.
                let retained = self.directory.keep();
                anyhow::bail!(
                    "Could not read the editor draft: {error}. The stored note has not been changed. Editor recovery files have been retained in the private directory {}",
                    retained.display()
                );
            }
        }
    }
}

pub async fn open_editor(file_path: &Path, settings: &Settings) -> anyhow::Result<()> {
    if !file_path.exists() {
        bail!("file doesn't exist: {}", file_path.display());
    }

    let file_path = file_path.canonicalize()?;

    let editor_raw = settings
        .external_editor
        .command
        .as_ref()
        .cloned()
        .or_else(|| get_git_editor().ok())
        .or_else(|| env::var(ENV_EDITOR_OPTIONS[0]).ok())
        .or_else(|| env::var(ENV_EDITOR_OPTIONS[1]).ok())
        .unwrap_or(String::from("vi"));

    let command = parse_command(&editor_raw)?;

    execute!(io::stdout(), DisableMouseCapture)?;
    defer! {
        if let Err(err) = enable_raw_mode() {
            log::error!("Restoring terminal raw mode after external editor failed: {err}");
        }
        if let Err(err) = execute!(io::stdout(), EnterAlternateScreen, EnableMouseCapture) {
            log::error!("Restoring terminal display after external editor failed: {err}");
        }
    }
    execute!(io::stdout(), LeaveAlternateScreen)?;
    disable_raw_mode()?;

    execute_command(&command, &file_path).await
}

fn parse_command(raw: &str) -> anyhow::Result<Vec<String>> {
    let command = shlex::split(raw)
        .ok_or_else(|| anyhow!("External editor command contains an unfinished quote or escape"))?;
    anyhow::ensure!(
        command.first().is_some_and(|program| !program.is_empty()),
        "External editor command is empty"
    );
    Ok(command)
}

async fn execute_command(command: &[String], file_path: &Path) -> anyhow::Result<()> {
    let status = Command::new(&command[0])
        .args(&command[1..])
        .arg(file_path)
        .status()
        .await
        .map_err(|err| anyhow!("Could not start external editor '{}': {err}", command[0]))?;
    anyhow::ensure!(
        status.success(),
        "External editor exited unsuccessfully ({status})"
    );
    Ok(())
}

/// Tries to get the configured git editor from Git global config.
fn get_git_editor() -> anyhow::Result<String> {
    let config = git2::Config::open_default()?;
    let editor = config.get_string("core.editor").map_err(|err| {
        log::trace!("Failed to retrieve git editor, Err: {err}");
        err
    })?;

    log::trace!("Git editor is: {editor}");

    Ok(editor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn editor_command_preserves_quoted_arguments_without_shell_expansion() {
        assert_eq!(
            parse_command(r#"'/Applications/My Editor' --wait --data-dir "/path with spaces" '' '$HOME;touch nope'"#).unwrap(),
            ["/Applications/My Editor", "--wait", "--data-dir", "/path with spaces", "", "$HOME;touch nope"]
        );
        for invalid in ["", "  ", "''", "editor 'unfinished", "editor \\"] {
            assert!(parse_command(invalid).is_err());
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn editor_failure_is_not_reported_as_success_and_keeps_its_output() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let editor = dir.path().join("editor");
        let note = dir.path().join("note with spaces");
        std::fs::write(
            &editor,
            "#!/bin/sh\nprintf %s 'partial draft' > \"$1\"\nexit 7\n",
        )
        .unwrap();
        std::fs::set_permissions(&editor, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::write(&note, "original").unwrap();
        let error = execute_command(&[editor.to_string_lossy().into_owned()], &note)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("unsuccessfully"));
        assert_eq!(std::fs::read_to_string(note).unwrap(), "partial draft");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn unreadable_editor_replacement_is_retained_inside_a_private_directory() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let draft = EditorDraft::new("original", "txt", Some(dir.path())).unwrap();
        let note = draft.path().to_owned();
        let recovery = note.parent().unwrap().to_owned();
        let editor = dir.path().join("editor");
        fs::write(&editor, "#!/bin/sh\nprintf '\\377' > \"$1.new\"\nchmod 644 \"$1.new\"\nmv \"$1.new\" \"$1\"\nexit 7\n").unwrap();
        fs::set_permissions(&editor, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(
            execute_command(&[editor.to_string_lossy().into_owned()], &note)
                .await
                .is_err()
        );
        let error = draft.read().unwrap_err();
        assert!(error.to_string().contains(recovery.to_str().unwrap()));
        assert_eq!(fs::read(&note).unwrap(), [0xff]);
        assert_eq!(
            fs::metadata(&note).unwrap().permissions().mode() & 0o777,
            0o644
        );
        assert_eq!(
            fs::metadata(&recovery).unwrap().permissions().mode() & 0o777,
            0o700
        );
    }

    #[cfg(unix)]
    #[test]
    fn editor_recovery_does_not_modify_symlink_targets() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        let draft = EditorDraft::new("original", "md", Some(dir.path())).unwrap();
        let outside = dir.path().join("outside");
        fs::write(&outside, "untouched").unwrap();
        fs::set_permissions(&outside, fs::Permissions::from_mode(0o644)).unwrap();
        fs::remove_file(draft.path()).unwrap();
        symlink(&outside, draft.path()).unwrap();
        assert!(draft.read().is_err());
        assert_eq!(fs::read_to_string(&outside).unwrap(), "untouched");
        assert_eq!(
            fs::metadata(outside).unwrap().permissions().mode() & 0o777,
            0o644
        );
    }
}

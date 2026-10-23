use std::{fs, io::Write};

use directories::BaseDirs;
use serde::{Deserialize, Serialize};

use super::*;

const STATE_FILE_NAME: &str = "state.json";
pub const DEFAULT_ENTRIES_LIST_PERCENTAGE: u16 = 30;
pub const MIN_ENTRIES_LIST_PERCENTAGE: u16 = 10;
pub const MAX_ENTRIES_LIST_PERCENTAGE: u16 = 60;

fn default_entries_list_percentage() -> u16 {
    DEFAULT_ENTRIES_LIST_PERCENTAGE
}

fn deserialize_entries_list_percentage<'de, D>(deserializer: D) -> Result<u16, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct PercentageVisitor;

    impl serde::de::Visitor<'_> for PercentageVisitor {
        type Value = u16;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an integer pane percentage")
        }

        fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<u16, E> {
            Ok(value.clamp(
                MIN_ENTRIES_LIST_PERCENTAGE.into(),
                MAX_ENTRIES_LIST_PERCENTAGE.into(),
            ) as u16)
        }

        fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<u16, E> {
            Ok(value.clamp(
                MIN_ENTRIES_LIST_PERCENTAGE.into(),
                MAX_ENTRIES_LIST_PERCENTAGE.into(),
            ) as u16)
        }
    }

    deserializer.deserialize_any(PercentageVisitor)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AppState {
    pub sorter: Sorter,
    pub full_screen: bool,
    #[serde(
        default = "default_entries_list_percentage",
        deserialize_with = "deserialize_entries_list_percentage"
    )]
    entries_list_percentage: u16,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            sorter: Sorter::default(),
            full_screen: false,
            entries_list_percentage: DEFAULT_ENTRIES_LIST_PERCENTAGE,
        }
    }
}

impl AppState {
    pub fn entries_list_percentage(&self) -> u16 {
        self.entries_list_percentage
    }

    pub fn set_entries_list_percentage(&mut self, percentage: u16) -> bool {
        let percentage = percentage.clamp(MIN_ENTRIES_LIST_PERCENTAGE, MAX_ENTRIES_LIST_PERCENTAGE);
        if self.entries_list_percentage == percentage {
            return false;
        }
        self.entries_list_percentage = percentage;
        true
    }

    pub fn load(settings: &Settings) -> anyhow::Result<Self> {
        // Move state file from legacy data directory to the new state directory
        // to avoid breaking changes on users.
        // TODO: Remove this after three releases.
        if settings.app_state_dir.is_none() {
            Self::move_legacy_state();
        }

        let state_path = Self::get_persist_path(settings)?;

        let state = if state_path.exists() {
            let state_file = File::open(state_path)
                .map_err(|err| anyhow!("Failed to load state file. Error info: {err}"))?;
            serde_json::from_reader(state_file)
                .map_err(|err| anyhow!("Failed to read state file. Error info: {err}"))?
        } else {
            AppState::default()
        };

        Ok(state)
    }

    fn get_persist_path(settings: &Settings) -> anyhow::Result<PathBuf> {
        if let Some(path) = settings.app_state_dir.as_ref() {
            Ok(path.join(STATE_FILE_NAME))
        } else {
            Self::default_persist_dir().map(|dir| dir.join(STATE_FILE_NAME))
        }
    }

    pub fn save(&self, settings: &Settings) -> anyhow::Result<()> {
        let mut state_path = Self::get_persist_path(settings)?;
        let mut followed_links = 0;
        loop {
            match fs::symlink_metadata(&state_path) {
                Ok(metadata) if metadata.is_symlink() => {
                    if followed_links == 40 {
                        bail!("Too many symbolic links in state file path");
                    }
                    let target = fs::read_link(&state_path)?;
                    state_path = if target.is_absolute() {
                        target
                    } else {
                        state_path
                            .parent()
                            .context("State file has no parent directory")?
                            .join(target)
                    };
                    followed_links += 1;
                }
                Ok(_) => {
                    state_path = fs::canonicalize(state_path)?;
                    break;
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                Err(error) => return Err(error.into()),
            }
        }
        if let Some(parent) = state_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let parent = state_path
            .parent()
            .context("State file has no parent directory")?;
        let mut staged = tempfile::NamedTempFile::new_in(parent)?;
        if let Ok(metadata) = fs::metadata(&state_path) {
            staged.as_file().set_permissions(metadata.permissions())?;
        }
        serde_json::to_writer_pretty(&mut staged, self)?;
        staged.flush()?;
        staged.as_file().sync_all()?;
        staged.persist(state_path)?;

        Ok(())
    }

    /// Return the default path of the directory used to persist the application state.
    /// It uses the state directories on supported platforms falling back to the data directory.
    pub fn default_persist_dir() -> anyhow::Result<PathBuf> {
        BaseDirs::new()
            .map(|base_dirs| {
                base_dirs
                    .state_dir()
                    .unwrap_or_else(|| base_dirs.data_dir())
                    .join("tui-journal")
            })
            .context("Config file path couldn't be retrieved")
    }

    /// Move app state from legacy path to the new one if the legacy exists and the new doesn't.
    fn move_legacy_state() {
        // Return early if operating system doesn't support `state_dir()`
        let state_path = match BaseDirs::new()
            .map(|base_dirs| base_dirs.state_dir().map(|state| state.join("tui-journal")))
        {
            Some(Some(state)) => state,
            _ => return,
        };

        // Gets legacy path which was used to store the state previously.
        let legacy_data_dir =
            match BaseDirs::new().map(|base_dirs| base_dirs.data_dir().join("tui-journal")) {
                Some(path) => path,
                None => return,
            };
        // Legacy already removed -> Done
        if !legacy_data_dir.exists() {
            return;
        }

        let legacy_state_file = legacy_data_dir.join(STATE_FILE_NAME);
        if !legacy_state_file.exists() {
            // Legacy dir exists but it has no files -> remove it -> Done.
            if let Err(err) = std::fs::remove_dir_all(&legacy_data_dir) {
                log::error!(
                    "Legacy State: Removing legacy directory failed. path: {}, Error {err}",
                    legacy_data_dir.display()
                );
            }
            return;
        }

        let new_state_file = state_path.join(STATE_FILE_NAME);
        if new_state_file.exists() {
            // New state file exists somehow -> Remove old state directory to avoid running this
            // again.
            if let Err(err) = std::fs::remove_dir_all(&legacy_data_dir) {
                log::error!(
                    "Legacy State: Removing legacy directory failed. path: {}, Error {err}",
                    legacy_data_dir.display()
                );
            }

            return;
        }

        if !state_path.exists() {
            // Create new state directory if not exists.
            if let Err(err) = std::fs::create_dir_all(&state_path) {
                log::error!(
                    "Legacy State: Creating state dir filed. Path: {}, Error {err}",
                    state_path.display()
                );
                return;
            }
        }

        // Move state file.
        if let Err(err) = std::fs::rename(legacy_state_file, new_state_file) {
            log::error!("Legacy State: Moving legacy state file failed. Error {err}");
            return;
        }

        // Finally remove legacy state directory.
        if let Err(err) = std::fs::remove_dir_all(&legacy_data_dir) {
            log::error!(
                "Legacy State: Removing legacy directory failed. path: {}, Error {err}",
                legacy_data_dir.display()
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn missing_state_loads_default() {
        let dir = tempfile::Builder::new()
            .prefix("state-empty")
            .tempdir()
            .unwrap();
        let settings = Settings {
            app_state_dir: Some(dir.path().to_path_buf()),
            ..Default::default()
        };

        let state = AppState::load(&settings).unwrap();

        assert!(!state.full_screen);
        assert_eq!(
            state.sorter.get_criteria(),
            Sorter::default().get_criteria()
        );
        assert!(matches!(state.sorter.order, SortOrder::Descending));
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::Builder::new()
            .prefix("state-save")
            .tempdir()
            .unwrap();
        let settings = Settings {
            app_state_dir: Some(dir.path().to_path_buf()),
            ..Default::default()
        };
        let mut sorter = Sorter::default();
        sorter.set_criteria(vec![SortCriteria::Title]);
        sorter.order = SortOrder::Ascending;

        let state = AppState {
            sorter,
            full_screen: true,
            entries_list_percentage: 45,
        };

        state.save(&settings).unwrap();
        let loaded = AppState::load(&settings).unwrap();

        assert!(loaded.full_screen);
        assert_eq!(loaded.entries_list_percentage(), 45);
        assert_eq!(loaded.sorter.get_criteria(), &[SortCriteria::Title]);
        assert!(matches!(loaded.sorter.order, SortOrder::Ascending));
    }

    #[test]
    fn custom_dir_sets_path() {
        let settings = Settings {
            app_state_dir: Some(PathBuf::from("/tmp/custom-state")),
            ..Default::default()
        };

        let state_path = AppState::get_persist_path(&settings).unwrap();

        assert_eq!(state_path, PathBuf::from("/tmp/custom-state/state.json"));
    }

    #[test]
    fn legacy_state_keeps_default_pane_size() {
        let mut value = serde_json::to_value(AppState::default()).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .remove("entries_list_percentage");
        let state: AppState = serde_json::from_value(value).unwrap();
        assert_eq!(state.entries_list_percentage(), 30);
    }

    #[test]
    fn stored_extreme_pane_sizes_are_clamped() {
        for (input, expected) in [(-500_i64, 10), (0, 10), (45, 45), (100_000, 60)] {
            let mut value = serde_json::to_value(AppState::default()).unwrap();
            value["entries_list_percentage"] = input.into();
            let state: AppState = serde_json::from_value(value).unwrap();
            assert_eq!(state.entries_list_percentage(), expected);
        }
    }

    #[cfg(unix)]
    #[test]
    fn save_preserves_state_symlink_and_permissions() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("actual.json");
        fs::write(&target, "{}").unwrap();
        fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
        symlink(&target, dir.path().join("state.json")).unwrap();
        let settings = Settings {
            app_state_dir: Some(dir.path().into()),
            ..Default::default()
        };
        let mut state = AppState::default();
        state.set_entries_list_percentage(45);
        state.save(&settings).unwrap();
        assert!(
            fs::symlink_metadata(dir.path().join("state.json"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::metadata(&target).unwrap().permissions().mode() & 0o777,
            0o640
        );
        assert_eq!(
            AppState::load(&settings).unwrap().entries_list_percentage(),
            45
        );
    }

    #[cfg(unix)]
    #[test]
    fn save_creates_missing_state_target_through_relative_symlinks() {
        use std::os::unix::fs::symlink;

        for chained in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            fs::create_dir(dir.path().join("targets")).unwrap();
            let target = dir.path().join("targets/actual.json");
            if chained {
                symlink("actual.json", dir.path().join("targets/link.json")).unwrap();
            }
            let destination = if chained {
                "targets/link.json"
            } else {
                "targets/actual.json"
            };
            let state_path = dir.path().join("state.json");
            symlink(destination, &state_path).unwrap();
            let settings = Settings {
                app_state_dir: Some(dir.path().into()),
                ..Default::default()
            };
            let mut state = AppState::default();
            state.set_entries_list_percentage(45);

            state.save(&settings).unwrap();

            assert!(target.is_file());
            assert!(fs::symlink_metadata(&state_path).unwrap().is_symlink());
            assert_eq!(
                fs::read_link(&state_path).unwrap(),
                PathBuf::from(destination)
            );
            if chained {
                assert_eq!(
                    fs::read_link(dir.path().join("targets/link.json")).unwrap(),
                    PathBuf::from("actual.json")
                );
            }
            assert_eq!(
                AppState::load(&settings).unwrap().entries_list_percentage(),
                45
            );
        }
    }

    #[cfg(unix)]
    #[test]
    fn save_rejects_cyclic_state_symlinks_without_replacing_them() {
        use std::os::unix::fs::symlink;

        let dir = tempfile::tempdir().unwrap();
        let state_path = dir.path().join("state.json");
        let other_path = dir.path().join("other.json");
        symlink("other.json", &state_path).unwrap();
        symlink("state.json", &other_path).unwrap();
        let settings = Settings {
            app_state_dir: Some(dir.path().into()),
            ..Default::default()
        };

        let error = AppState::default().save(&settings).unwrap_err();

        assert!(error.to_string().contains("Too many symbolic links"));
        assert_eq!(
            fs::read_link(state_path).unwrap(),
            PathBuf::from("other.json")
        );
        assert_eq!(
            fs::read_link(other_path).unwrap(),
            PathBuf::from("state.json")
        );
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn failed_state_replacement_preserves_existing_target() {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("state.json");
        fs::create_dir(&target).unwrap();
        fs::write(target.join("keep"), "untouched").unwrap();
        let settings = Settings {
            app_state_dir: Some(dir.path().into()),
            ..Default::default()
        };
        assert!(AppState::default().save(&settings).is_err());
        assert_eq!(
            fs::read_to_string(target.join("keep")).unwrap(),
            "untouched"
        );
    }
}

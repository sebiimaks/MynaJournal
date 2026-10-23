use std::path::PathBuf;

use backend::FileFormat;
use serde::{Deserialize, Serialize};

use super::get_default_data_dir;

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct FileBackend {
    #[serde(default)]
    pub directory: Option<PathBuf>,
    #[serde(default)]
    pub format: FileFormat,
}

pub fn get_default_file_path() -> anyhow::Result<PathBuf> {
    Ok(get_default_data_dir()?.join("entries"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{BackendType, Settings};

    #[test]
    fn plain_files_are_the_default_and_have_their_own_directory() {
        let settings: Settings = toml::from_str("backend_type = 'File'").unwrap();
        assert_eq!(settings.backend_type, Some(BackendType::File));
        assert_eq!(settings.file_backend.directory, None);
        assert_eq!(settings.file_backend.format, FileFormat::Plain);
        assert_eq!(
            get_default_file_path().unwrap(),
            get_default_data_dir().unwrap().join("entries")
        );
    }

    #[test]
    fn markdown_and_custom_directory_survive_completing_and_serializing_settings() {
        let mut settings: Settings = toml::from_str(
            "backend_type = 'File'\n[file_backend]\ndirectory = '/tmp/notes'\nformat = 'markdown'",
        )
        .unwrap();
        let text = settings.get_as_text().unwrap();
        let settings: Settings = toml::from_str(&text).unwrap();
        assert_eq!(
            settings.file_backend.directory,
            Some(PathBuf::from("/tmp/notes"))
        );
        assert_eq!(settings.file_backend.format, FileFormat::Markdown);
    }

    #[test]
    fn invalid_file_format_is_rejected() {
        assert!(toml::from_str::<Settings>("[file_backend]\nformat = 'html'").is_err());
    }

    #[cfg(not(any(feature = "sqlite", feature = "json", feature = "vjournal")))]
    #[test]
    fn file_only_build_defaults_to_file_backend() {
        assert_eq!(BackendType::default(), BackendType::File);
    }
}

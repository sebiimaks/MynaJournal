use std::{fs, process::Command};

#[test]
fn licenses_work_with_invalid_config_without_creating_app_files() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("invalid.toml");
    let invalid_config = "this is not valid TOML [";
    fs::write(&config, invalid_config).unwrap();

    let mut command = Command::new(env!("CARGO_BIN_EXE_mjournal"));
    command
        .current_dir(root.path())
        .env("HOME", root.path())
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .env("XDG_DATA_HOME", root.path().join("data"))
        .env("XDG_CACHE_HOME", root.path().join("cache"))
        .arg("--config")
        .arg(&config)
        .arg("--log")
        .arg(root.path().join("logs/journal.log"));
    #[cfg(feature = "file")]
    command
        .arg("--file-dir-path")
        .arg(root.path().join("journals"));

    let output = command.arg("licenses").output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let notices = String::from_utf8(output.stdout).unwrap();
    assert!(notices.contains(include_str!("../LICENSE")));
    assert!(notices.contains(include_str!("../ATTRIBUTION.md")));
    assert!(notices.contains(include_str!("../THIRD_PARTY_NOTICES.txt")));
    assert!(notices.contains("Copyright (c) 2023 Ammar Abou Zor"));
    assert_eq!(fs::read_to_string(&config).unwrap(), invalid_config);
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 1);
}

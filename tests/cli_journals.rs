#[cfg(all(feature = "json", feature = "file"))]
#[test]
fn inactive_cli_path_does_not_create_directories() {
    let dir = tempfile::tempdir().unwrap();
    let inactive = dir.path().join("archived-profile/data/entries");
    let active = dir.path().join("active/entries.json");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_mjournal"))
        .arg("--config")
        .arg(dir.path())
        .arg("--json-file-path")
        .arg(&active)
        .arg("--file-dir-path")
        .arg(&inactive)
        .args(["--backend-type", "json", "--log"])
        .arg(dir.path().join("app.log"))
        .arg("print-config")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(active.parent().unwrap().is_dir());
    assert!(!dir.path().join("archived-profile").exists());
    let settings: toml::Value = toml::from_str(&String::from_utf8(output.stdout).unwrap()).unwrap();
    assert_eq!(settings["backend_type"].as_str(), Some("Json"));
    assert_eq!(
        settings["file_backend"]["directory"].as_str(),
        inactive.to_str()
    );
}

#[cfg(feature = "json")]
#[test]
fn log_destination_cannot_overwrite_journal_or_wrapped_key() {
    let directory = tempfile::tempdir().unwrap();
    let native = directory.path().join("entries.json");
    let header = directory.path().join("entries.json.myna-lock.json");
    let sentinel = vec![b'x'; 2048];
    for target in [&native, &header] {
        std::fs::write(target, &sentinel).unwrap();
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_mjournal"))
            .arg("--config")
            .arg(directory.path())
            .arg("--json-file-path")
            .arg(&native)
            .arg("--log")
            .arg(target)
            .arg("print-config")
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert_eq!(std::fs::read(target).unwrap(), sentinel);
    }
    #[cfg(unix)]
    {
        let alias = directory.path().join("innocent.log");
        std::fs::hard_link(&header, &alias).unwrap();
        let result = std::process::Command::new(env!("CARGO_BIN_EXE_mjournal"))
            .arg("--config")
            .arg(directory.path())
            .arg("--json-file-path")
            .arg(&native)
            .arg("--log")
            .arg(alias)
            .arg("print-config")
            .output()
            .unwrap();
        assert!(!result.status.success());
        assert_eq!(std::fs::read(&header).unwrap(), sentinel);
    }
}

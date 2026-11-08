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

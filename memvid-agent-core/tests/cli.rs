//! CLI-level smoke tests for the packaged binary.
//!
//! These tests do not need a GGUF model: both flags exit before the model is
//! loaded or downloaded.

use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_aten-ia")
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(bin())
        .args(args)
        .output()
        .expect("failed to launch aten-ia binary")
}

#[test]
fn version_flag_prints_version_and_exits() {
    let out = run(&["--version"]);
    assert!(
        out.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("aten-ia"), "unexpected stdout: {stdout}");
    assert!(stdout.contains(env!("CARGO_PKG_VERSION")));
}

#[test]
fn self_test_flag_runs_full_smoke_suite() {
    let out = run(&["--self-test"]);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "self-test failed\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    assert!(
        stdout.contains("autodiagnóstico"),
        "missing banner: {stdout}"
    );
    assert!(
        stdout.contains("todo correcto"),
        "missing success: {stdout}"
    );
}

#[test]
fn verify_alias_behaves_like_self_test() {
    let out = run(&["--verify"]);
    assert!(
        out.status.success(),
        "verify failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(String::from_utf8_lossy(&out.stdout).contains("todo correcto"));
}

#[test]
fn purge_data_removes_config_data_model_and_empty_models_dir() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("data")).unwrap();
    std::fs::create_dir_all(dir.path().join("models")).unwrap();
    std::fs::write(dir.path().join("data/index.jsonl"), "{}").unwrap();
    std::fs::write(dir.path().join("models/test.gguf"), b"fake").unwrap();
    std::fs::write(
        dir.path().join("config.json"),
        r#"{"data_dir":"data","model":{"path":"models/test.gguf"}}"#,
    )
    .unwrap();

    let out = Command::new(bin())
        .args(["--purge-data", "--yes"])
        .current_dir(dir.path())
        .output()
        .expect("failed to run purge");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "purge failed\nstdout:\n{stdout}\nstderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!dir.path().join("config.json").exists());
    assert!(!dir.path().join("data").exists());
    assert!(!dir.path().join("models/test.gguf").exists());
    assert!(!dir.path().join("models").exists());
    assert!(stdout.contains("Limpieza completada"));
}

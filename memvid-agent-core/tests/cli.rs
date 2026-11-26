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

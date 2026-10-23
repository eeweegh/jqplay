use std::io::Write;
use std::process::{Command, Stdio};

fn jqplay() -> Command {
    Command::new(env!("CARGO_BIN_EXE_jqplay"))
}

#[test]
fn version_includes_package_version() {
    let out = jqplay().arg("--version").output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.starts_with(&format!("jqplay {}", env!("CARGO_PKG_VERSION"))), "{text}");
}

#[test]
fn help_documents_keybindings() {
    let out = jqplay().arg("--help").output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    for needle in ["--filter", "--theme", "--history-file", "Ctrl+Q", "Readline editing", "solarized-dark"] {
        assert!(text.contains(needle), "missing {needle}");
    }
}

#[test]
fn color_and_no_color_conflict() {
    let out = jqplay().args(["--color", "--no-color"]).output().unwrap();
    assert!(!out.status.success());
}

#[test]
fn missing_file_reports_error() {
    let out = jqplay().arg("/nonexistent/file.json").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    let err = String::from_utf8(out.stderr).unwrap();
    assert!(err.starts_with("jqplay: error: could not read /nonexistent/file.json"), "{err}");
}

#[test]
fn bad_config_reports_error() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = dir.path().join("c.toml");
    std::fs::write(&cfg, "nope = 1").unwrap();
    let out = jqplay().arg("--config").arg(&cfg).output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert!(String::from_utf8(out.stderr).unwrap().contains("invalid config"));
}

#[test]
fn missing_jq_is_reported() {
    let out = jqplay().env("PATH", "/nonexistent").output().unwrap();
    assert_eq!(out.status.code(), Some(1));
    assert_eq!(
        String::from_utf8(out.stderr).unwrap(),
        "jqplay: error: 'jq' is required but was not found on PATH\n"
    );
}

#[test]
fn completions_and_man_page_are_generated() {
    let out = jqplay().args(["completions", "zsh"]).output().unwrap();
    assert!(String::from_utf8(out.stdout).unwrap().contains("#compdef jqplay"));
    let out = jqplay().arg("man").output().unwrap();
    assert!(String::from_utf8(out.stdout).unwrap().contains(".TH jqplay"));
}

#[test]
fn old_generator_flags_are_rejected() {
    assert!(!jqplay().arg("--man").output().unwrap().status.success());
    assert!(!jqplay().args(["--completions", "zsh"]).output().unwrap().status.success());
}

#[test]
fn piped_stdin_without_a_terminal_exits_cleanly_when_tty_is_unavailable() {
    // No controlling terminal under the test harness: startup must fail with an error, not hang.
    let mut child = jqplay()
        .args(["--history-file", ""])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child.stdin.take().unwrap().write_all(b"{}").unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(!out.status.success() || !out.stdout.is_empty());
}

fn assert_silent_success(mut cmd: Command, stdin: Option<&[u8]>) {
    let mut child = cmd
        .args(["--history-file", ""])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut pipe = child.stdin.take().unwrap();
    if let Some(data) = stdin {
        pipe.write_all(data).unwrap();
    }
    drop(pipe);
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "{:?}", out.status);
    assert!(out.stdout.is_empty() && out.stderr.is_empty(), "{out:?}");
}

#[test]
fn empty_stdin_exits_silently_with_status_zero() {
    assert_silent_success(jqplay(), None);
    assert_silent_success(jqplay(), Some(b" \n\t\n"));
}

#[test]
fn empty_file_exits_silently_with_status_zero() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("empty.json");
    std::fs::write(&file, "\n").unwrap();
    let mut cmd = jqplay();
    cmd.arg(&file);
    assert_silent_success(cmd, None);
}

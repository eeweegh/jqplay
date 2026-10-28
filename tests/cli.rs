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

fn run_piped(mut cmd: Command, stdin: &[u8]) -> std::process::Output {
    let mut child = cmd
        .args(["--history-file", ""])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut pipe = child.stdin.take().unwrap();
    let _ = pipe.write_all(stdin); // the child may exit without reading (file argument)
    drop(pipe);
    child.wait_with_output().unwrap()
}

#[test]
fn stdin_is_copied_to_stdout_when_stdout_is_not_a_terminal() {
    // Also covers non-UTF-8 bytes and a missing jq: nothing is parsed or run.
    let data = b"{\"a\": 1}\n\xff\xfe not json\n";
    let mut cmd = jqplay();
    cmd.env("PATH", "/nonexistent");
    let out = run_piped(cmd, data);
    assert!(out.status.success(), "{:?}", out.status);
    assert_eq!(out.stdout, data);
    assert!(out.stderr.is_empty());
}

#[test]
fn file_is_copied_to_stdout_when_stdout_is_not_a_terminal() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("in.json");
    std::fs::write(&file, "{\"b\": [1, 2]}\n").unwrap();
    let mut cmd = jqplay();
    cmd.arg(&file);
    let out = run_piped(cmd, b"ignored");
    assert!(out.status.success());
    assert_eq!(out.stdout, b"{\"b\": [1, 2]}\n");
}

#[test]
fn empty_stdin_copies_nothing_when_stdout_is_not_a_terminal() {
    let out = run_piped(jqplay(), b"");
    assert!(out.status.success());
    assert!(out.stdout.is_empty() && out.stderr.is_empty(), "{out:?}");
}

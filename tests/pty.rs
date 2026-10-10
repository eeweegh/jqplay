#![cfg(unix)]
use expectrl::{Eof, Expect, Session};
use std::process::Command;
use std::time::Duration;

const CTRL_Q: &str = "\x11";
const CTRL_C: &str = "\x03";

fn spawn(args: &[&str], stdin_json: Option<&str>) -> expectrl::session::OsSession {
    let bin = env!("CARGO_BIN_EXE_jqplay");
    let mut cmd = match stdin_json {
        Some(json) => {
            let mut c = Command::new("sh");
            c.arg("-c").arg(format!("printf '%s' '{json}' | exec \"$0\" \"$@\"")).arg(bin);
            c
        }
        None => Command::new(bin),
    };
    cmd.args(args).args(["--history-file", ""]).env("TERM", "xterm-256color");
    let mut session = Session::spawn(cmd).expect("spawn in pty");
    session.set_expect_timeout(Some(Duration::from_secs(10)));
    session
}

fn pause() {
    std::thread::sleep(Duration::from_millis(500));
}

#[test]
fn ctrl_q_prints_filter_and_exits_zero() {
    let mut s = spawn(&["--filter", ".a", "tests/data/small.json"], None);
    pause();
    s.send(CTRL_Q).unwrap();
    s.expect("jq '.a'").unwrap();
    s.expect(Eof).unwrap();
}

#[test]
fn ctrl_c_exits_without_printing_filter() {
    let mut s = spawn(&["--filter", ".a", "tests/data/small.json"], None);
    pause();
    s.send(CTRL_C).unwrap();
    let rest = s.expect(Eof).unwrap();
    assert!(!String::from_utf8_lossy(rest.as_bytes()).contains("jq '.a'"));
}

#[test]
fn typing_updates_result_pane() {
    let mut s = spawn(&["tests/data/small.json"], None);
    pause();
    s.send(".b").unwrap();
    s.expect("\"hello\"").unwrap();
    s.send(CTRL_Q).unwrap();
    s.expect("jq '.b'").unwrap();
}

#[test]
fn stdin_pipe_still_receives_keyboard_input() {
    let mut s = spawn(&["--filter", ".b"], Some("{\"a\":1,\"b\":\"piped\"}"));
    s.expect("piped").unwrap();
    s.send(CTRL_Q).unwrap();
    s.expect("jq '.b'").unwrap();
    s.expect(Eof).unwrap();
}

#[test]
fn tab_opens_path_completion() {
    let mut s = spawn(&["tests/data/small.json"], None);
    pause();
    s.send(".").unwrap();
    s.send("\t").unwrap();
    s.expect("jq paths").unwrap();
    s.send(CTRL_C).unwrap();
}

#[test]
fn ctrl_s_opens_save_prompt_in_a_real_terminal() {
    let mut s = spawn(&["--filter", ".a", "tests/data/small.json"], None);
    pause();
    s.send("\x13").unwrap();
    s.expect("save results to path").unwrap();
    s.send(CTRL_C).unwrap();
}

#![cfg(unix)]
use expectrl::{Eof, Expect, Session};
use std::ops::{Deref, DerefMut};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};
use std::time::Duration;

const CTRL_Q: &str = "\x11";
const CTRL_C: &str = "\x03";

// forkpty from several test threads at once fails sporadically, so pty tests run one at a time.
static PTY_LOCK: Mutex<()> = Mutex::new(());

struct Pty {
    session: expectrl::session::OsSession,
    _guard: MutexGuard<'static, ()>,
}

impl Deref for Pty {
    type Target = expectrl::session::OsSession;
    fn deref(&self) -> &Self::Target {
        &self.session
    }
}

impl DerefMut for Pty {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.session
    }
}

fn spawn(args: &[&str], stdin_json: Option<&str>) -> Pty {
    let bin = env!("CARGO_BIN_EXE_jqplay");
    let cmd = match stdin_json {
        Some(json) => {
            let mut c = Command::new("sh");
            c.arg("-c").arg(format!("printf '%s' '{json}' | exec \"$0\" \"$@\"")).arg(bin);
            c
        }
        None => Command::new(bin),
    };
    start(cmd, args)
}

fn start(mut cmd: Command, args: &[&str]) -> Pty {
    let guard = PTY_LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    cmd.args(args).args(["--history-file", ""]).env("TERM", "xterm-256color");
    let mut session = Session::spawn(cmd).expect("spawn in pty");
    session.set_expect_timeout(Some(Duration::from_secs(10)));
    Pty { session, _guard: guard }
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

#[test]
fn ctrl_q_accepts_and_prints_filter() {
    let mut s = spawn(&["--filter", ".a", "tests/data/small.json"], None);
    pause();
    s.send("\x11").unwrap();
    s.expect("jq '.a'").unwrap();
    s.expect(Eof).unwrap();
}

#[test]
fn q_in_a_view_pane_accepts() {
    let mut s = spawn(&["--filter", ".a", "tests/data/small.json"], None);
    pause();
    s.send("\t").unwrap();
    s.send("q").unwrap();
    s.expect("jq '.a'").unwrap();
    s.expect(Eof).unwrap();
}

#[test]
fn ctrl_space_opens_completion_manually() {
    let mut s = spawn(&["tests/data/small.json"], None);
    pause();
    s.send("\x00").unwrap();
    s.expect("jq paths").unwrap();
    s.send(CTRL_C).unwrap();
}

#[test]
fn enter_on_popup_selection_does_not_quit() {
    let mut s = spawn(&["tests/data/small.json"], None);
    pause();
    s.send(".").unwrap();
    s.expect("jq paths").unwrap();
    s.send("\r").unwrap();
    pause();
    s.send("\x03").unwrap();
    let rest = s.expect(Eof).unwrap();
    assert!(!String::from_utf8_lossy(rest.before()).contains("jq '"));
}

#[test]
fn ctrl_j_inserts_a_newline_and_ctrl_q_accepts() {
    let mut s = spawn(&["tests/data/small.json"], None);
    pause();
    s.send(".a").unwrap();
    s.send("\x0a").unwrap();
    s.send("| .b").unwrap();
    pause();
    s.send("\x11").unwrap();
    s.expect("| .b'").unwrap();
    s.expect(Eof).unwrap();
}

#[test]
fn alt_enter_inserts_a_newline_in_a_real_terminal() {
    let mut s = spawn(&["tests/data/small.json"], None);
    pause();
    s.send(".a").unwrap();
    s.send("\x1b\r").unwrap();
    s.send("| .b").unwrap();
    pause();
    s.send("\x11").unwrap();
    s.expect("| .b'").unwrap();
}

// Whether a jq process whose command line contains `marker` is running.
fn jq_running(marker: &str) -> bool {
    Command::new("pgrep").args(["-f", &format!("jq .*{marker}")]).output().is_ok_and(|o| o.status.success())
}

#[test]
fn quitting_stops_a_jq_that_never_ends() {
    let marker = format!("jqplay-orphan-{}", std::process::id());
    let filter = format!("until(false; .) | \"{marker}\"");
    // The shell outlives jqplay like in a real terminal; with jqplay as the pty's session leader, closing
    // the pty would hang up the stray jq and hide the problem.
    let mut shell = Command::new("sh");
    shell.arg("-c").arg("\"$0\" \"$@\"; sleep 8").arg(env!("CARGO_BIN_EXE_jqplay"));
    let mut s = start(shell, &["--filter", &filter, "tests/data/small.json"]);
    std::thread::sleep(Duration::from_secs(1));
    assert!(jq_running(&marker), "the runaway jq never started, so the test proves nothing");
    s.send(CTRL_Q).unwrap();
    s.expect(format!("jq '{filter}'")).unwrap();
    for _ in 0..40 {
        if !jq_running(&marker) {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    panic!("jq outlived jqplay");
}

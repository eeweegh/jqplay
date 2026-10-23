use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

// A jq run that takes longer than this is killed, so a filter that never ends cannot hold its slot forever.
pub const JOB_TIMEOUT: Duration = Duration::from_secs(60);

// Path-enumeration expression shared with generic/bin/jqpaths so completions match that tool.
pub const JQPATHS_EXPR: &str = r#"
[
  path(..)  |
  map(
    select(type == "string" and (test("[^a-zA-Z0-9_]") or test("^[0-9]"))) |= "\"" + . + "\""
  ) |
  map(
    select(type == "number") |= "[]"
  ) | join(".")
] | sort | unique | .[] | split(".[") | join("[") | "." + .
"#;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Options {
    pub raw: bool,
    pub compact: bool,
    pub sort_keys: bool,
    pub slurp: bool,
    pub color: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Output {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn ok(&self) -> bool {
        self.code == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Stop {
    Cancelled,
    TimedOut,
}

#[derive(Debug, Default)]
struct Running {
    child: Option<Child>,
    stop: Option<Stop>,
}

// Lets whoever started a jq run stop it: a request that supersedes it, quitting, or the timeout watchdog.
#[derive(Debug, Clone, Default)]
pub struct Cancel(Arc<Mutex<Running>>);

impl Cancel {
    pub fn cancel(&self) {
        self.stop(Stop::Cancelled);
    }

    pub fn was_cancelled(&self) -> bool {
        self.lock().stop == Some(Stop::Cancelled)
    }

    fn stop(&self, why: Stop) {
        let mut running = self.lock();
        running.stop.get_or_insert(why);
        if let Some(child) = running.child.as_mut() {
            let _ = child.kill();
        }
    }

    // A stop requested before the child existed still takes effect.
    fn attach(&self, mut child: Child) {
        let mut running = self.lock();
        if running.stop.is_some() {
            let _ = child.kill();
        }
        running.child = Some(child);
    }

    fn lock(&self) -> MutexGuard<'_, Running> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

pub fn is_available() -> bool {
    Command::new("jq").arg("--version").stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok()
}

pub fn build_argv(filter: &str, options: Options) -> Vec<String> {
    let mut argv = vec![if options.color { "-C" } else { "-M" }.to_string()];
    for (on, flag) in
        [(options.raw, "-r"), (options.compact, "-c"), (options.sort_keys, "-S"), (options.slurp, "-s")]
    {
        if on {
            argv.push(flag.to_string());
        }
    }
    // jq reads an argument starting with '-' as an option; a leading space makes it the program (the
    // portable way: jq 1.6 does not accept `--` before the program).
    argv.push(if filter.starts_with('-') { format!(" {filter}") } else { filter.to_string() });
    argv
}

// Feeds stdin from a thread so large inputs and outputs cannot deadlock on full pipes.
fn run(args: &[String], input: &str, cancel: &Cancel, timeout: Duration) -> Output {
    let spawned = Command::new("jq")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => {
            return Output { code: -1, stdout: String::new(), stderr: format!("failed to run jq: {e}") };
        }
    };
    let mut stdin = child.stdin.take().expect("piped stdin");
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    cancel.attach(child);
    // Dropping `done` wakes the watchdog, which then ends without killing anything.
    let (done, finished) = mpsc::channel::<()>();
    let watchdog = cancel.clone();
    let (out_buf, err_buf) = thread::scope(|scope| {
        scope.spawn(move || {
            if finished.recv_timeout(timeout) == Err(RecvTimeoutError::Timeout) {
                watchdog.stop(Stop::TimedOut);
            }
        });
        scope.spawn(move || stdin.write_all(input.as_bytes()));
        let err_reader = scope.spawn(move || {
            let mut buf = Vec::new();
            let _ = stderr.read_to_end(&mut buf);
            buf
        });
        let mut out_buf = Vec::new();
        let _ = stdout.read_to_end(&mut out_buf);
        drop(done);
        (out_buf, err_reader.join().unwrap_or_default())
    });
    let child = cancel.lock().child.take();
    let code = child.and_then(|mut c| c.wait().ok()).and_then(|s| s.code()).unwrap_or(-1);
    let stopped = |stderr: String| Output { code: -1, stdout: String::new(), stderr };
    match cancel.lock().stop {
        Some(Stop::Cancelled) => stopped("jq was cancelled".into()),
        Some(Stop::TimedOut) => stopped(format!("jq timed out after {}s", timeout.as_secs())),
        None => Output {
            code,
            stdout: String::from_utf8_lossy(&out_buf).into_owned(),
            stderr: String::from_utf8_lossy(&err_buf).into_owned(),
        },
    }
}

// Items of the input: a lone top-level array counts its elements, otherwise the top-level values.
const COUNT_EXPR: &str = r#"if length == 1 and (.[0] | type) == "array" then .[0] | length else length end"#;

// Succeeds iff the input parses; stdout then holds the item count.
pub fn validate(json: &str, cancel: &Cancel) -> Output {
    run(&["-e".into(), "-s".into(), COUNT_EXPR.into()], json, cancel, JOB_TIMEOUT)
}

pub fn run_filter(json: &str, filter: &str, options: Options, cancel: &Cancel) -> Output {
    run(&build_argv(filter, options), json, cancel, JOB_TIMEOUT)
}

pub fn pretty(json: &str, slurp: bool, cancel: &Cancel) -> Output {
    run_filter(json, ".", Options { slurp, color: true, ..Options::default() }, cancel)
}

pub fn paths(json: &str, slurp: bool, cancel: &Cancel) -> Result<Vec<String>, String> {
    let mut args = vec!["-r".to_string()];
    if slurp {
        args.push("-s".into());
    }
    args.push(JQPATHS_EXPR.into());
    let out = run(&args, json, cancel, JOB_TIMEOUT);
    if !out.ok() {
        return Err(out.stderr);
    }
    let mut seen = std::collections::HashSet::new();
    Ok(out.stdout.lines().filter(|l| seen.insert(*l)).map(str::to_string).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argv_orders_flags_then_filter() {
        let o = Options { raw: true, slurp: true, ..Options::default() };
        assert_eq!(build_argv(".a", o), ["-M", "-r", "-s", ".a"]);
        let o = Options { color: true, compact: true, sort_keys: true, ..Options::default() };
        assert_eq!(build_argv(".", o), ["-C", "-c", "-S", "."]);
    }

    #[test]
    fn a_filter_starting_with_a_dash_is_not_an_option() {
        assert_eq!(build_argv("-1", Options::default()), ["-M", " -1"]);
        if is_available() {
            let out = run_filter("{}", "-h", Options::default(), &Cancel::default());
            assert!(!out.stdout.contains("Usage"), "{}", out.stdout);
            assert_eq!(run_filter("5", "-1 + 3", Options::default(), &Cancel::default()).stdout, "2\n");
        }
    }

    #[test]
    fn a_never_ending_filter_is_stopped_by_cancel_and_by_timeout() {
        if !is_available() {
            return;
        }
        let cancel = Cancel::default();
        let canceller = cancel.clone();
        let stopper = thread::spawn(move || {
            thread::sleep(Duration::from_millis(200));
            canceller.cancel();
        });
        let out = run_filter("1", "until(false; .)", Options::default(), &cancel);
        stopper.join().unwrap();
        assert!(!out.ok() && cancel.was_cancelled(), "{out:?}");

        let cancel = Cancel::default();
        let args = build_argv("until(false; .)", Options::default());
        let out = run(&args, "1", &cancel, Duration::from_millis(200));
        assert!(!out.ok() && !cancel.was_cancelled());
        assert_eq!(out.stderr, "jq timed out after 0s");

        // A stop requested before the run starts is honoured too.
        let cancel = Cancel::default();
        cancel.cancel();
        assert!(!run_filter("1", "until(false; .)", Options::default(), &cancel).ok());
    }

    #[test]
    fn validate_and_run_against_real_jq() {
        if !is_available() {
            return;
        }
        assert!(validate("{\"a\":1}", &Cancel::default()).ok());
        assert!(!validate("{oops", &Cancel::default()).ok());
        let out = run_filter(
            "{\"a\":[1,2]}",
            ".a[]",
            Options { compact: true, ..Options::default() },
            &Cancel::default(),
        );
        assert_eq!(out.stdout, "1\n2\n");
    }

    #[test]
    fn validate_counts_items() {
        if !is_available() {
            return;
        }
        for (input, count) in
            [("[1,2,3]", "3"), ("{\"a\":1}", "1"), ("{\"a\":1}\n{\"b\":2}", "2"), ("[]", "0"), ("null", "1")]
        {
            let out = validate(input, &Cancel::default());
            assert!(out.ok(), "{input}: {}", out.stderr);
            assert_eq!(out.stdout.trim(), count, "{input}");
        }
    }

    #[test]
    fn paths_are_deduplicated_across_documents() {
        if !is_available() {
            return;
        }
        let got = paths("{\"a\":{\"b\":1}}\n{\"a\":{\"b\":2}}", false, &Cancel::default()).unwrap();
        assert!(got.contains(&".a".to_string()));
        assert_eq!(got.iter().filter(|p| p.as_str() == ".a.b").count(), 1);
    }
}

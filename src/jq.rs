use std::io::{Read, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::thread;
use std::time::Duration;

// A jq run that takes longer than this is killed, so a filter that never ends cannot hold its slot forever.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

// Lists every path in the input as a jq-style accessor (`.a.b`, `.a[]`, `.a."odd key"`), for completion.
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

// How a jq run ended; for everything but `Exited` the message is in `Output::stderr`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Exited(i32),
    // Ended by a signal nobody here sent.
    Killed,
    SpawnFailed,
    TimedOut,
    // Stopped because a newer request superseded it, or because the app is quitting.
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    pub status: Status,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    pub fn ok(&self) -> bool {
        self.status == Status::Exited(0)
    }

    pub fn cancelled(&self) -> bool {
        self.status == Status::Cancelled
    }

    fn failed(status: Status, message: String) -> Self {
        Self { status, stdout: String::new(), stderr: message }
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

// Belongs to one jq run and lets whoever started it stop it: a request that supersedes it, quitting, or the
// timeout watchdog. Carries the run's time limit.
#[derive(Debug, Clone)]
pub struct Handle {
    running: Arc<Mutex<Running>>,
    timeout: Duration,
}

impl Default for Handle {
    fn default() -> Self {
        Self::new(DEFAULT_TIMEOUT)
    }
}

impl Handle {
    pub fn new(timeout: Duration) -> Self {
        Self { running: Arc::default(), timeout }
    }

    pub fn cancel(&self) {
        self.stop(Stop::Cancelled);
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
        self.running.lock().unwrap_or_else(PoisonError::into_inner)
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
fn run(args: &[String], input: &str, handle: &Handle) -> Output {
    let spawned = Command::new("jq")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(e) => return Output::failed(Status::SpawnFailed, format!("failed to run jq: {e}")),
    };
    let mut stdin = child.stdin.take().expect("piped stdin");
    let mut stdout = child.stdout.take().expect("piped stdout");
    let mut stderr = child.stderr.take().expect("piped stderr");
    handle.attach(child);
    // Dropping `done` wakes the watchdog, which then ends without killing anything.
    let (done, finished) = mpsc::channel::<()>();
    let (watchdog, timeout) = (handle.clone(), handle.timeout);
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
    let child = handle.lock().child.take();
    let exit = child.and_then(|mut c| c.wait().ok()).and_then(|s| s.code());
    let stop = handle.lock().stop;
    match (stop, exit) {
        (Some(Stop::Cancelled), _) => Output::failed(Status::Cancelled, "jq was cancelled".into()),
        (Some(Stop::TimedOut), _) => {
            Output::failed(Status::TimedOut, format!("jq timed out after {}s", timeout.as_secs()))
        }
        (None, exit) => Output {
            status: exit.map_or(Status::Killed, Status::Exited),
            stdout: String::from_utf8_lossy(&out_buf).into_owned(),
            stderr: String::from_utf8_lossy(&err_buf).into_owned(),
        },
    }
}

// Items of the input: a lone top-level array counts its elements, otherwise the top-level values.
const COUNT_EXPR: &str = r#"if length == 1 and (.[0] | type) == "array" then .[0] | length else length end"#;

// Succeeds iff the input parses; stdout then holds the item count.
pub fn validate(json: &str, handle: &Handle) -> Output {
    run(&["-e".into(), "-s".into(), COUNT_EXPR.into()], json, handle)
}

pub fn run_filter(json: &str, filter: &str, options: Options, handle: &Handle) -> Output {
    run(&build_argv(filter, options), json, handle)
}

pub fn pretty(json: &str, slurp: bool, handle: &Handle) -> Output {
    run_filter(json, ".", Options { slurp, color: true, ..Options::default() }, handle)
}

// Every distinct path in the input; the failing run is returned as is.
pub fn paths(json: &str, slurp: bool, handle: &Handle) -> Result<Vec<String>, Output> {
    let mut args = vec!["-r".to_string()];
    if slurp {
        args.push("-s".into());
    }
    args.push(JQPATHS_EXPR.into());
    let out = run(&args, json, handle);
    if !out.ok() {
        return Err(out);
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
            let out = run_filter("{}", "-h", Options::default(), &Handle::default());
            assert!(!out.stdout.contains("Usage"), "{}", out.stdout);
            assert_eq!(run_filter("5", "-1 + 3", Options::default(), &Handle::default()).stdout, "2\n");
        }
    }

    #[test]
    fn a_never_ending_filter_is_stopped_by_cancel_and_by_timeout() {
        if !is_available() {
            return;
        }
        let handle = Handle::default();
        let canceller = handle.clone();
        let stopper = thread::spawn(move || {
            thread::sleep(Duration::from_millis(200));
            canceller.cancel();
        });
        let out = run_filter("1", "until(false; .)", Options::default(), &handle);
        stopper.join().unwrap();
        assert!(out.cancelled(), "{out:?}");

        let handle = Handle::new(Duration::from_millis(200));
        let out = run_filter("1", "until(false; .)", Options::default(), &handle);
        assert_eq!(out.status, Status::TimedOut);
        assert_eq!(out.stderr, "jq timed out after 0s");

        // A stop requested before the run starts is honoured too.
        let handle = Handle::default();
        handle.cancel();
        assert!(run_filter("1", "until(false; .)", Options::default(), &handle).cancelled());
    }

    #[test]
    fn exit_codes_are_reported() {
        if !is_available() {
            return;
        }
        let out = run_filter("{}", ".a | error(\"boom\")", Options::default(), &Handle::default());
        assert_eq!(out.status, Status::Exited(5));
        assert!(out.stderr.contains("boom"));
    }

    #[test]
    fn validate_and_run_against_real_jq() {
        if !is_available() {
            return;
        }
        assert!(validate("{\"a\":1}", &Handle::default()).ok());
        assert!(!validate("{oops", &Handle::default()).ok());
        let out = run_filter(
            "{\"a\":[1,2]}",
            ".a[]",
            Options { compact: true, ..Options::default() },
            &Handle::default(),
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
            let out = validate(input, &Handle::default());
            assert!(out.ok(), "{input}: {}", out.stderr);
            assert_eq!(out.stdout.trim(), count, "{input}");
        }
    }

    #[test]
    fn paths_are_deduplicated_across_documents() {
        if !is_available() {
            return;
        }
        let got = paths("{\"a\":{\"b\":1}}\n{\"a\":{\"b\":2}}", false, &Handle::default()).unwrap();
        assert!(got.contains(&".a".to_string()));
        assert_eq!(got.iter().filter(|p| p.as_str() == ".a.b").count(), 1);
    }
}

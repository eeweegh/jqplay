use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::thread;

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
    argv.push(filter.to_string());
    argv
}

// Feeds stdin from a thread so large inputs and outputs cannot deadlock on full pipes.
fn run(args: &[String], input: &str) -> Output {
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
    let data = input.as_bytes().to_vec();
    let writer = thread::spawn(move || stdin.write_all(&data));
    let err_reader = thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = stderr.read_to_end(&mut buf);
        buf
    });
    let mut out_buf = Vec::new();
    let _ = stdout.read_to_end(&mut out_buf);
    let _ = writer.join();
    let err_buf = err_reader.join().unwrap_or_default();
    let code = child.wait().ok().and_then(|s| s.code()).unwrap_or(-1);
    Output {
        code,
        stdout: String::from_utf8_lossy(&out_buf).into_owned(),
        stderr: String::from_utf8_lossy(&err_buf).into_owned(),
    }
}

pub fn validate(json: &str) -> Output {
    run(&["-e".into(), ".".into()], json)
}

pub fn run_filter(json: &str, filter: &str, options: Options) -> Output {
    run(&build_argv(filter, options), json)
}

pub fn pretty(json: &str, slurp: bool) -> Output {
    run_filter(json, ".", Options { slurp, color: true, ..Options::default() })
}

pub fn paths(json: &str, slurp: bool) -> Result<Vec<String>, String> {
    let mut args = vec!["-r".to_string()];
    if slurp {
        args.push("-s".into());
    }
    args.push(JQPATHS_EXPR.into());
    let out = run(&args, json);
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
    fn validate_and_run_against_real_jq() {
        if !is_available() {
            return;
        }
        assert!(validate("{\"a\":1}").ok());
        assert!(!validate("{oops").ok());
        let out = run_filter("{\"a\":[1,2]}", ".a[]", Options { compact: true, ..Options::default() });
        assert_eq!(out.stdout, "1\n2\n");
    }

    #[test]
    fn paths_are_deduplicated_across_documents() {
        if !is_available() {
            return;
        }
        let got = paths("{\"a\":{\"b\":1}}\n{\"a\":{\"b\":2}}", false).unwrap();
        assert!(got.contains(&".a".to_string()));
        assert_eq!(got.iter().filter(|p| p.as_str() == ".a.b").count(), 1);
    }
}

# jqplay

A terminal UI for exploring JSON with [jq](https://jqlang.github.io/jq/), inspired by jqplay.org.
Rust rewrite (ratatui) of the original Python/urwid tool; all JSON work is delegated to the `jq` binary.

```
jqplay data.json
cat data.json | jqplay
jqplay data.json --filter '.items[] | select(.active)'
```

`jqplay --help` lists every key binding. `q` (JSON/Result panes) or Ctrl+Q (anywhere) accepts and prints `jq '<filter>'`; Ctrl+C aborts. Empty input (empty stdin or file) exits silently with status 0. When stdout is not a terminal (e.g. `cat data.json | jqplay | less`), jqplay shows no UI and copies its input to stdout unchanged.
The filter pane is multi-line: Enter (also Alt+Enter, Ctrl+J) adds a line and the pane grows with it.

## Layout of the code

| Module | Role |
| --- | --- |
| `cli`, `config` | clap definitions; TOML config (`~/.config/jqplay/config.toml`) merged under the flags |
| `jq`, `job` | jq subprocess runner (cancellable, with a timeout) and the job/reply types for the worker threads |
| `app` | pure state machine: events in, jobs out (no I/O except history, reload, save); split into `jobs` (caches, in-flight bookkeeping), `keys`, `mouse`, `pane`, `complete` and `save` |
| `ui`, `theme` | stateless ratatui rendering of `&mut App` |
| `ansi` | SGR parser and the line-indexed `Document` that only parses visible rows |
| `editor`, `completion`, `history` | filter line editing, path completion, history file |
| `input`, `terminal`, `run` | stdin/tty handling, terminal modes, the event loop |
| `util` | small numeric helpers (saturating `u16`, wrap-around stepping) |

## Development

```
just check      # fmt, clippy -D warnings, tests
just perf          # generates a synthetic ~5MB file with jq and opens it
```

Tests: unit tests per module, `tests/ui.rs` (insta snapshots on ratatui's `TestBackend`, jq run
synchronously), `tests/cli.rs` (binary behavior) and `tests/pty.rs` (a real pty: Ctrl+Q/Ctrl+C,
piped stdin, completion). `jq` must be on PATH.

The version lives in `Cargo.toml` and is reported by `--version` together with the git commit. `just bump 1.2.3` sets it, commits, tags `v1.2.3` and pushes both.
Shell completions and the man page are generated: `jqplay completions zsh`, `jqplay man`.

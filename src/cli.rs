use crate::theme::ThemeName;
use clap::Parser;
use clap_complete::Shell;
use std::path::PathBuf;

const VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), " (", env!("JQPLAY_GIT_SHA"), ")");

const AFTER_HELP: &str = "\
Prerequisites:
  jq   required. Used for all JSON validation/formatting and filter execution.

Layout:
  +----------------------------------------------------------+
  |                    jq Filter (top, short)                |
  +------------------------------+---------------------------+
  |     JSON Input (left)        |      Result (right)       |
  +------------------------------+---------------------------+
  Each pane scrolls independently (arrows, PageUp/PageDown, Home/End, mouse
  wheel). JSON Input and Result are view-only; load JSON via a file argument,
  stdin, or Ctrl+F. On a clean exit (Ctrl+Q) the current filter is printed to
  stdout as: jq '<filter>'

Global keybindings:
  Ctrl+N / Ctrl+B   switch focus between panes (next / previous)
  Ctrl+Q            clean exit: code 0, prints the current jq filter
  Ctrl+C            hard exit: code 1, prints nothing
  Ctrl+S            save the Result text to a path (any pane); Ctrl+Y also
                    does this from the JSON or Result pane
  Esc               dismiss a status message / close the completion popup
  Ctrl+F            reload the JSON from its source file (not for stdin)
  Up / Down         (filter pane, popup closed) recall previous/next filter

Option toggles (JSON Input or Result pane only, so they never clash with typing):
  r                 raw output (-r)            c   compact output (-c)
  S                 sort keys (-S)             s   slurp (-s)
  C                 syntax coloring

jq-key completion (filter pane only):
  Tab / Shift+Tab   open the popup / cycle through matches
  Up / Down         move the selection; Enter accepts, Esc closes
  Typing narrows the list; a boundary character (space, |, (, ...) closes it.

Readline editing (filter pane only; overrides Ctrl+Y save there; use Ctrl+S):
  Ctrl+A / Ctrl+E   start / end of line      Ctrl+K   kill to end of line
  Ctrl+D            delete char under cursor Ctrl+U   kill to start of line
  Ctrl+W            kill previous word       Ctrl+Y   yank last kill
  Ctrl+T            transpose two characters

Config file (TOML, optional): ~/.config/jqplay/config.toml with keys theme,
raw, compact, sort_keys, slurp, color, history_file, mouse. Flags override it.

Examples:
  jqplay data.json
  cat data.json | jqplay
  jqplay data.json --filter '.items[] | select(.active)'";

#[derive(Debug, Parser)]
#[command(name = "jqplay", version = VERSION, about = "A terminal UI for exploring JSON with jq, inspired by jqplay.org.", after_help = AFTER_HELP)]
pub struct Cli {
    /// JSON file to preload (omit to read from stdin, if piped)
    pub file: Option<PathBuf>,
    /// Initial jq filter
    #[arg(long)]
    pub filter: Option<String>,
    /// Color theme, set at startup only [default: solarized-dark]
    #[arg(long, value_enum)]
    pub theme: Option<ThemeName>,
    /// Start with raw output (-r) enabled
    #[arg(long)]
    pub raw: bool,
    /// Start with compact output (-c) enabled
    #[arg(long)]
    pub compact: bool,
    /// Start with sort-keys output (-S) enabled
    #[arg(long)]
    pub sort_keys: bool,
    /// Start with slurp (-s) enabled, for NDJSON/concatenated JSON input
    #[arg(long)]
    pub slurp: bool,
    /// Force syntax coloring on at startup, even for a large file
    #[arg(long, conflicts_with = "no_color")]
    pub color: bool,
    /// Force syntax coloring off at startup (default: on, except inputs over 200KB)
    #[arg(long)]
    pub no_color: bool,
    /// Cross-session filter history file; an empty string disables it [default: ~/.local/state/jqplay/history]
    #[arg(long, value_name = "PATH")]
    pub history_file: Option<String>,
    /// Disable mouse handling
    #[arg(long)]
    pub no_mouse: bool,
    /// Read settings from this TOML file instead of the default location
    #[arg(long, value_name = "PATH")]
    pub config: Option<PathBuf>,
    /// Print a shell completion script and exit
    #[arg(long, value_name = "SHELL", hide = true)]
    pub completions: Option<Shell>,
    /// Print the man page (roff) and exit
    #[arg(long, hide = true)]
    pub man: bool,
}

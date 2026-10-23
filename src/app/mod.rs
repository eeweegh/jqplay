mod complete;
mod jobs;
mod keys;
mod mouse;
mod pane;
mod save;

use crate::completion::{self, Completion};
use crate::config::Settings;
use crate::editor::LineEditor;
use crate::history::FilterHistory;
use crate::job::JobKind;
use crate::job::{Job, Reply};
use crate::jq::{Options, Output};
use crate::theme::Theme;
pub use completion::MAX_VISIBLE_ROWS;
use jobs::{Cache, InFlight, InputKey, RunKey};
pub use mouse::split_for_column;
pub use pane::{Pane, Search, SearchMode};
use ratatui::crossterm::event::{Event as TermEvent, KeyEventKind};
use ratatui::layout::Rect;
pub use save::Prompt;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const DEBOUNCE: Duration = Duration::from_millis(300);

#[derive(Debug)]
pub enum Event {
    Term(TermEvent),
    Reply(Reply),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Clean(String),
    Abort,
    // Empty input: nothing to explore, so the program exits silently with status 0.
    NoInput,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    Filter,
    Json,
    Results,
}

impl Focus {
    const ORDER: [Self; 3] = [Self::Filter, Self::Json, Self::Results];

    fn step(self, delta: isize) -> Self {
        let i = Self::ORDER.iter().position(|f| *f == self).unwrap_or(0);
        Self::ORDER[crate::util::cycle(i, delta, Self::ORDER.len())]
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Rects {
    pub filter: Rect,
    pub json: Rect,
    pub results: Rect,
}

#[derive(Debug)]
pub struct App {
    pub theme: Theme,
    pub options: Options,
    pub focus: Focus,
    pub editor: LineEditor,
    pub completion: Completion,
    pub prompt: Option<Prompt>,
    pub json_pane: Pane,
    pub results_pane: Pane,
    pub json_valid: bool,
    pub json_count: Option<usize>,
    pub autopair: bool,
    pub autocomplete: bool,
    pub status_message: String,
    pub rects: Rects,
    pub split_pct: u16,
    suspend: bool,
    dragging: bool,
    history: FilterHistory,
    history_file: Option<PathBuf>,
    source_file: Option<PathBuf>,
    last_save_path: Option<String>,
    json: Arc<str>,
    generation: u64,
    applied_filter: String,
    debounce_deadline: Option<Instant>,
    completion_pending: bool,
    timeout: Duration,
    validity: Cache<u64, Output>,
    pretty: Cache<InputKey, Output>,
    run: Cache<RunKey, Output>,
    paths: Cache<InputKey, Result<Vec<String>, Output>>,
    inflight: [Option<InFlight>; JobKind::SLOTS],
    outbox: Vec<Job>,
    outcome: Option<Outcome>,
}

// Quitting must not leave a runaway jq behind.
impl Drop for App {
    fn drop(&mut self) {
        for running in self.inflight.iter().flatten() {
            running.handle.cancel();
        }
    }
}

impl App {
    pub fn new(settings: &Settings, json: Option<String>) -> Self {
        let history = settings.history_file.as_deref().map(FilterHistory::load).unwrap_or_default();
        let filter = settings.filter.clone().unwrap_or_default();
        let has_json = json.as_deref().is_some_and(|j| !j.is_empty());
        let mut app = Self {
            theme: Theme::named(settings.theme),
            options: settings.options,
            focus: if has_json { Focus::Filter } else { Focus::Json },
            editor: LineEditor::new(&filter),
            completion: Completion::default(),
            prompt: None,
            json_pane: Pane::default(),
            results_pane: Pane::default(),
            json_valid: true,
            json_count: None,
            autopair: settings.autopair,
            autocomplete: settings.autocomplete,
            status_message: String::new(),
            rects: Rects::default(),
            split_pct: 50,
            suspend: false,
            dragging: false,
            history,
            history_file: settings.history_file.clone(),
            source_file: settings.file.clone(),
            last_save_path: None,
            json: Arc::from(json.unwrap_or_default()),
            generation: 0,
            applied_filter: filter,
            debounce_deadline: None,
            completion_pending: false,
            timeout: settings.timeout,
            validity: Cache::default(),
            pretty: Cache::default(),
            run: Cache::default(),
            paths: Cache::default(),
            inflight: Default::default(),
            outbox: Vec::new(),
            outcome: None,
        };
        app.refresh();
        app
    }

    pub fn outcome(&self) -> Option<&Outcome> {
        self.outcome.as_ref()
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.debounce_deadline
    }

    pub fn tick(&mut self, now: Instant) {
        if self.debounce_deadline.is_some_and(|d| now >= d) {
            self.run_now();
        }
    }

    pub fn handle(&mut self, event: Event) {
        match event {
            Event::Reply(reply) => self.on_reply(reply),
            Event::Term(TermEvent::Key(key)) if key.kind != KeyEventKind::Release => self.on_key(&key),
            Event::Term(TermEvent::Mouse(mouse)) => self.on_mouse(mouse),
            Event::Term(TermEvent::Paste(text)) => self.on_paste(&text),
            Event::Term(_) => {}
        }
    }

    // Writes the history array back; called once when the app quits normally (not on Ctrl-C).
    pub fn save_history(&mut self) {
        if let Some(path) = &self.history_file {
            self.history.save(path, self.editor.text());
        }
    }
}

use crate::ansi::{Document, strip_ansi};
use crate::completion::{self, CANDIDATE_LIMIT, Completion, find_current_token, is_token_char, match_paths};
use crate::config::Settings;
use crate::editor::LineEditor;
use crate::history::FilterHistory;
use crate::job::{Job, JobKind, JobResult, Reply};
use crate::jq::Options;
use crate::paths::expand_tilde;
use crate::theme::Theme;
use ratatui::crossterm::event::{
    Event as TermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::layout::{Position, Rect};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const DEBOUNCE: Duration = Duration::from_millis(300);
const WHEEL_STEP: usize = 3;
const DEFAULT_SAVE_PATH: &str = "result.json";

#[derive(Debug)]
pub enum Event {
    Term(TermEvent),
    Reply(Reply),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    Clean(String),
    Abort,
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
        let i = Self::ORDER.iter().position(|f| *f == self).unwrap_or(0) as isize;
        Self::ORDER[(i + delta).rem_euclid(3) as usize]
    }
}

#[derive(Debug, Default)]
pub struct Pane {
    pub doc: Document,
    pub error: bool,
    pub scroll_y: usize,
    pub scroll_x: usize,
    pub view_h: usize,
}

impl Pane {
    // Replaces the content, returning to the top only when it actually changed.
    fn set(&mut self, doc: Document, error: bool) {
        if doc.text() != self.doc.text() || error != self.error {
            self.scroll_y = 0;
            self.scroll_x = 0;
        }
        self.doc = doc;
        self.error = error;
    }

    fn key(&mut self, code: KeyCode) {
        let page = self.view_h.saturating_sub(1).max(1);
        match code {
            KeyCode::Up => self.scroll_y = self.scroll_y.saturating_sub(1),
            KeyCode::Down => self.scroll_y += 1,
            KeyCode::PageUp => self.scroll_y = self.scroll_y.saturating_sub(page),
            KeyCode::PageDown => self.scroll_y += page,
            KeyCode::Home => (self.scroll_y, self.scroll_x) = (0, 0),
            KeyCode::End => self.scroll_y = usize::MAX,
            KeyCode::Left => self.scroll_x = self.scroll_x.saturating_sub(4),
            KeyCode::Right => self.scroll_x += 4,
            _ => {}
        }
    }

    fn wheel(&mut self, down: bool) {
        self.scroll_y = if down {
            self.scroll_y.saturating_add(WHEEL_STEP)
        } else {
            self.scroll_y.saturating_sub(WHEEL_STEP)
        };
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub struct Rects {
    pub filter: Rect,
    pub json: Rect,
    pub results: Rect,
}

#[derive(Debug)]
pub struct Prompt {
    pub editor: LineEditor,
    text: String,
}

type RunKey = (u64, String, Options);

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
    pub autopair: bool,
    pub autocomplete: bool,
    pub status_message: String,
    pub rects: Rects,
    history: FilterHistory,
    history_file: Option<PathBuf>,
    source_file: Option<PathBuf>,
    last_save_path: Option<String>,
    json: Arc<str>,
    generation: u64,
    applied_filter: String,
    debounce_deadline: Option<Instant>,
    completion_pending: bool,
    validity: Option<(u64, crate::jq::Output)>,
    pretty: Option<(u64, bool, crate::jq::Output)>,
    run: Option<(RunKey, crate::jq::Output)>,
    paths: Option<(u64, bool, Result<Vec<String>, String>)>,
    inflight: [Option<(u64, JobKind)>; JobKind::SLOTS],
    outbox: Vec<Job>,
    outcome: Option<Outcome>,
}

fn is_plain_char(key: &KeyEvent) -> Option<char> {
    match key.code {
        KeyCode::Char(c) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => Some(c),
        _ => None,
    }
}

fn ctrl_char(key: &KeyEvent) -> Option<char> {
    match key.code {
        KeyCode::Char(c)
            if key.modifiers.contains(KeyModifiers::CONTROL)
                && !key.modifiers.contains(KeyModifiers::ALT) =>
        {
            Some(c.to_ascii_lowercase())
        }
        _ => None,
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Edited {
    No,
    Moved,
    Changed,
}

// Editing keys shared by the filter line and the save prompt.
fn apply_edit_key(ed: &mut LineEditor, key: &KeyEvent, autopair: bool) -> Edited {
    if let Some(c) = ctrl_char(key) {
        return match c {
            'a' => {
                ed.home();
                Edited::Moved
            }
            'e' => {
                ed.end();
                Edited::Moved
            }
            'd' => {
                ed.delete();
                Edited::Changed
            }
            'k' => {
                ed.kill_to_end();
                Edited::Changed
            }
            'u' => {
                ed.kill_to_start();
                Edited::Changed
            }
            'w' => {
                ed.kill_word();
                Edited::Changed
            }
            'y' => {
                ed.yank();
                Edited::Changed
            }
            't' => {
                ed.transpose();
                Edited::Changed
            }
            _ => Edited::No,
        };
    }
    if let Some(c) = is_plain_char(key) {
        ed.insert_char(c, autopair);
        return Edited::Changed;
    }
    match key.code {
        KeyCode::Backspace => {
            if autopair {
                ed.backspace_pair();
            } else {
                ed.backspace();
            }
            Edited::Changed
        }
        KeyCode::Delete => {
            ed.delete();
            Edited::Changed
        }
        KeyCode::Left => {
            ed.left();
            Edited::Moved
        }
        KeyCode::Right => {
            ed.right();
            Edited::Moved
        }
        KeyCode::Home => {
            ed.home();
            Edited::Moved
        }
        KeyCode::End => {
            ed.end();
            Edited::Moved
        }
        _ => Edited::No,
    }
}

fn error_doc(stderr: &str, fallback: &str) -> Document {
    Document::plain(if stderr.trim().is_empty() { fallback } else { stderr })
}

impl App {
    pub fn new(settings: &Settings, json: Option<String>) -> Self {
        let history = settings.history_file.as_deref().map(FilterHistory::load).unwrap_or_default();
        let filter = settings.filter.clone().or_else(|| history.last().map(String::from)).unwrap_or_default();
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
            autopair: settings.autopair,
            autocomplete: settings.autocomplete,
            status_message: String::new(),
            rects: Rects::default(),
            history,
            history_file: settings.history_file.clone(),
            source_file: settings.file.clone(),
            last_save_path: None,
            json: Arc::from(json.unwrap_or_default()),
            generation: 0,
            applied_filter: filter,
            debounce_deadline: None,
            completion_pending: false,
            validity: None,
            pretty: None,
            run: None,
            paths: None,
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

    pub fn working(&self) -> bool {
        self.inflight.iter().any(Option::is_some)
    }

    pub fn deadline(&self) -> Option<Instant> {
        self.debounce_deadline
    }

    pub fn take_jobs(&mut self) -> Vec<Job> {
        std::mem::take(&mut self.outbox)
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

    // -- jobs and caches ------------------------------------------------------

    fn blank_json(&self) -> bool {
        self.json.trim().is_empty()
    }

    fn run_key(&self) -> RunKey {
        (self.generation, self.applied_filter.clone(), Options { color: true, ..self.options })
    }

    fn want(&mut self, kind: JobKind) {
        let slot = kind.slot();
        if self.inflight[slot].is_none() {
            self.inflight[slot] = Some((self.generation, kind.clone()));
            self.outbox.push(Job { generation: self.generation, json: Arc::clone(&self.json), kind });
        }
    }

    // Requests whatever the current state needs and the caches lack.
    fn request_missing(&mut self) {
        if self.blank_json() {
            return;
        }
        let (generation, slurp) = (self.generation, self.options.slurp);
        if self.validity.as_ref().is_none_or(|(g, _)| *g != generation) {
            self.want(JobKind::Validate);
        }
        if self.pretty.as_ref().is_none_or(|(g, s, _)| (*g, *s) != (generation, slurp)) {
            self.want(JobKind::Pretty { slurp });
        }
        if !self.applied_filter.trim().is_empty()
            && self.run.as_ref().is_none_or(|(k, _)| *k != self.run_key())
        {
            let options = Options { color: true, ..self.options };
            self.want(JobKind::Run { filter: self.applied_filter.clone(), options });
        }
        if self.paths.as_ref().is_none_or(|(g, s, _)| (*g, *s) != (generation, slurp)) {
            self.want(JobKind::Paths { slurp });
        }
    }

    fn refresh(&mut self) {
        self.rebuild_json_pane();
        self.rebuild_results();
        self.request_missing();
    }

    fn on_reply(&mut self, reply: Reply) {
        self.inflight[reply.kind.slot()] = None;
        if reply.generation == self.generation {
            match (reply.kind, reply.result) {
                (JobKind::Validate, JobResult::Output(out)) => self.validity = Some((reply.generation, out)),
                (JobKind::Pretty { slurp }, JobResult::Output(out)) if slurp == self.options.slurp => {
                    self.pretty = Some((reply.generation, slurp, out));
                }
                (JobKind::Run { filter, options }, JobResult::Output(out)) => {
                    let key = (reply.generation, filter, options);
                    if key == self.run_key() {
                        self.run = Some((key, out));
                    }
                }
                (JobKind::Paths { slurp }, JobResult::Paths(res)) if slurp == self.options.slurp => {
                    self.paths = Some((reply.generation, slurp, res));
                    if std::mem::take(&mut self.completion_pending) && self.focus == Focus::Filter {
                        self.open_completion();
                    }
                }
                _ => {}
            }
        }
        self.refresh();
    }

    fn rebuild_json_pane(&mut self) {
        if self.blank_json() {
            return self.json_pane.set(Document::default(), false);
        }
        let Some((g, s, out)) = &self.pretty else { return };
        if (*g, *s) != (self.generation, self.options.slurp) {
            return;
        }
        let (doc, error) = if out.ok() {
            (Document::new(&out.stdout, self.options.color), false)
        } else {
            (error_doc(&out.stderr, "jq error"), true)
        };
        self.json_pane.set(doc, error);
    }

    fn rebuild_results(&mut self) {
        if self.blank_json() {
            self.json_valid = true;
            return self.results_pane.set(Document::default(), false);
        }
        let Some((g, validity)) = &self.validity else { return };
        if *g != self.generation {
            return;
        }
        self.json_valid = validity.ok();
        if !validity.ok() {
            let doc = error_doc(&validity.stderr, "invalid JSON");
            return self.results_pane.set(doc, true);
        }
        if self.applied_filter.trim().is_empty() {
            return self.results_pane.set(Document::default(), false);
        }
        let Some((key, out)) = &self.run else { return };
        if *key != self.run_key() {
            return;
        }
        if out.ok() {
            let doc = Document::new(&out.stdout, self.options.color);
            self.results_pane.set(doc, false);
            self.record_history();
        } else {
            self.results_pane.set(error_doc(&out.stderr, "jq error"), true);
        }
    }

    fn record_history(&mut self) {
        let filter = self.applied_filter.clone();
        self.history.record(&filter);
        if let Some(path) = &self.history_file {
            self.history.save_session_entry(path, &filter);
        }
    }

    fn run_now(&mut self) {
        self.debounce_deadline = None;
        self.applied_filter = self.editor.text().to_string();
        self.refresh();
    }

    fn on_filter_change(&mut self) {
        self.debounce_deadline = Some(Instant::now() + DEBOUNCE);
    }

    // -- options / reload / save ------------------------------------------------

    fn toggle(&mut self, flip: impl FnOnce(&mut Options)) {
        let before = self.options;
        flip(&mut self.options);
        if before.slurp != self.options.slurp || before.color != self.options.color {
            self.rebuild_json_pane();
        }
        self.run_now();
    }

    fn reload_source(&mut self) {
        let Some(path) = self.source_file.clone() else {
            return self.status_message = "no source file to reload (loaded from stdin)".into();
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => {
                self.json = Arc::from(text);
                self.generation += 1;
                self.status_message = format!("reloaded {}", path.display());
                self.run_now();
            }
            Err(e) => self.status_message = format!("reload failed: {e}"),
        }
    }

    fn current_results_text(&self) -> Option<String> {
        if self.blank_json() || !self.json_valid || self.editor.text().trim().is_empty() {
            return None;
        }
        let (key, out) = self.run.as_ref()?;
        (*key == self.run_key() && out.ok()).then(|| strip_ansi(&out.stdout))
    }

    fn open_save_prompt(&mut self) {
        let Some(text) = self.current_results_text() else {
            return self.status_message = "nothing to save (no valid results)".into();
        };
        let initial = self.last_save_path.as_deref().unwrap_or(DEFAULT_SAVE_PATH);
        self.prompt = Some(Prompt { editor: LineEditor::new(initial), text });
    }

    fn confirm_save(&mut self, prompt: &Prompt) {
        let path = prompt.editor.text().trim().to_string();
        let target = expand_tilde(&path);
        let saved = target
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|()| std::fs::write(&target, &prompt.text));
        self.status_message = match saved {
            Ok(()) => {
                self.last_save_path = Some(path.clone());
                format!("saved to {path}")
            }
            Err(e) => format!("save failed: {e}"),
        };
    }

    // -- completion -------------------------------------------------------------

    fn open_completion(&mut self) {
        if self.blank_json() {
            return self.status_message = "no JSON loaded".into();
        }
        let candidates = match &self.paths {
            Some((g, s, res)) if (*g, *s) == (self.generation, self.options.slurp) => match res {
                Ok(c) => c.clone(),
                Err(e) => return self.status_message = e.clone(),
            },
            _ => {
                self.status_message = "looking up jq paths...".into();
                self.completion_pending = true;
                return;
            }
        };
        let (start, token) = find_current_token(self.editor.text(), self.editor.cursor());
        let filtered = match_paths(&candidates, &token, CANDIDATE_LIMIT);
        if filtered.is_empty() {
            return self.status_message = format!("no jq paths match {token:?}");
        }
        self.status_message.clear();
        self.completion = Completion { open: true, candidates, filtered, selected: 0, token_start: start };
    }

    // Silent variant for typing: only for path tokens, only with loaded paths, never sets a message.
    fn auto_open_completion(&mut self) {
        let Some((generation, slurp, Ok(candidates))) = &self.paths else { return };
        if (*generation, *slurp) != (self.generation, self.options.slurp) {
            return;
        }
        let (start, token) = find_current_token(self.editor.text(), self.editor.cursor());
        let filtered = match_paths(candidates, &token, CANDIDATE_LIMIT);
        if !token.starts_with('.') || filtered.is_empty() || filtered == [token] {
            return;
        }
        let candidates = candidates.clone();
        self.completion = Completion { open: true, candidates, filtered, selected: 0, token_start: start };
    }

    fn refresh_completion(&mut self) {
        let (start, token) = find_current_token(self.editor.text(), self.editor.cursor());
        let filtered = match_paths(&self.completion.candidates, &token, CANDIDATE_LIMIT);
        if filtered.is_empty() || filtered == [token] {
            return self.completion.close();
        }
        self.completion.token_start = start;
        self.completion.filtered = filtered;
        self.completion.selected = 0;
    }

    fn accept_completion(&mut self) {
        let Some(choice) = self.completion.choice().map(String::from) else { return };
        let chars: Vec<char> = self.editor.text().chars().collect();
        let (start, cursor) = (self.completion.token_start, self.editor.cursor());
        let new_text: String = chars[..start]
            .iter()
            .chain(choice.chars().collect::<Vec<_>>().iter())
            .chain(chars[cursor..].iter())
            .collect();
        self.editor.set_text(&new_text);
        self.editor.set_cursor(start + choice.chars().count());
        self.completion.close();
        self.on_filter_change();
    }

    // -- input ------------------------------------------------------------------

    fn set_focus(&mut self, focus: Focus) {
        self.completion.close();
        self.focus = focus;
    }

    fn on_paste(&mut self, text: &str) {
        if self.focus != Focus::Filter && self.prompt.is_none() {
            return;
        }
        if let Some(p) = &mut self.prompt {
            p.editor.insert(&text.replace(['\r', '\n'], " "));
        } else {
            self.editor.insert(&text.replace("\r\n", "\n").replace('\r', "\n"));
            self.on_filter_change();
            if self.completion.open {
                self.refresh_completion();
            }
        }
    }

    fn on_mouse(&mut self, mouse: MouseEvent) {
        if self.prompt.is_some() {
            return;
        }
        let at = Position::new(mouse.column, mouse.row);
        let target = [
            (Focus::Filter, self.rects.filter),
            (Focus::Json, self.rects.json),
            (Focus::Results, self.rects.results),
        ]
        .into_iter()
        .find(|(_, r)| r.contains(at))
        .map(|(f, _)| f);
        match (mouse.kind, target) {
            (MouseEventKind::Down(MouseButton::Left), Some(f)) if f != self.focus => self.set_focus(f),
            (MouseEventKind::ScrollUp | MouseEventKind::ScrollDown, Some(f)) => {
                let down = mouse.kind == MouseEventKind::ScrollDown;
                match f {
                    Focus::Json => self.json_pane.wheel(down),
                    Focus::Results => self.results_pane.wheel(down),
                    Focus::Filter => {}
                }
            }
            _ => {}
        }
    }

    fn on_key(&mut self, key: &KeyEvent) {
        if self.prompt.is_some() {
            return self.prompt_key(key);
        }
        match self.focus {
            Focus::Filter => {
                if self.filter_key(key) {
                    return;
                }
            }
            Focus::Json | Focus::Results => {
                if self.option_key(key) {
                    return;
                }
                let pane =
                    if self.focus == Focus::Json { &mut self.json_pane } else { &mut self.results_pane };
                pane.key(key.code);
            }
        }
        self.global_key(key);
    }

    fn prompt_key(&mut self, key: &KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                if let Some(prompt) = self.prompt.take() {
                    self.confirm_save(&prompt);
                }
            }
            KeyCode::Esc => self.prompt = None,
            _ => {
                if let Some(p) = &mut self.prompt {
                    apply_edit_key(&mut p.editor, key, false);
                }
            }
        }
    }

    // Returns true when the filter line consumed the key.
    fn filter_key(&mut self, key: &KeyEvent) -> bool {
        let newline = (key.code == KeyCode::Enter
            && key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::SHIFT))
            || ctrl_char(key) == Some('j');
        if newline {
            self.editor.insert_newline();
            self.completion.close();
            self.on_filter_change();
            return true;
        }
        let open = self.completion.open;
        match key.code {
            KeyCode::Up | KeyCode::Down if open => {
                self.completion.step(if key.code == KeyCode::Up { -1 } else { 1 });
                return true;
            }
            KeyCode::Enter if open => {
                self.accept_completion();
                return true;
            }
            KeyCode::Up | KeyCode::Down => {
                let up = key.code == KeyCode::Up;
                if (up && self.editor.up()) || (!up && self.editor.down()) {
                    return true;
                }
                let recalled =
                    if up { self.history.recall_up(self.editor.text()) } else { self.history.recall_down() };
                if let Some(text) = recalled {
                    self.editor.set_text(&text);
                }
                self.on_filter_change();
                return true;
            }
            KeyCode::Enter => {
                self.accept();
                return true;
            }
            _ => {}
        }
        if open && is_plain_char(key).is_some_and(|c| !is_token_char(c)) {
            self.completion.close();
        }
        match apply_edit_key(&mut self.editor, key, self.autopair) {
            Edited::No => return false,
            Edited::Moved => {}
            Edited::Changed => self.on_filter_change(),
        }
        if self.completion.open {
            self.refresh_completion();
        } else if self.autocomplete && is_plain_char(key).is_some_and(is_token_char) {
            self.auto_open_completion();
        }
        true
    }

    fn accept(&mut self) {
        self.outcome = Some(Outcome::Clean(self.editor.text().to_string()));
    }

    // Single-key option toggles (plus `q` to accept); only reachable from the JSON and Result panes.
    fn option_key(&mut self, key: &KeyEvent) -> bool {
        match is_plain_char(key) {
            Some('r') => self.toggle(|o| o.raw = !o.raw),
            Some('c') => self.toggle(|o| o.compact = !o.compact),
            Some('S') => self.toggle(|o| o.sort_keys = !o.sort_keys),
            Some('s') => self.toggle(|o| o.slurp = !o.slurp),
            Some('C') => self.toggle(|o| o.color = !o.color),
            Some('q') => self.accept(),
            _ => return false,
        }
        true
    }

    fn global_key(&mut self, key: &KeyEvent) {
        if let Some(c) = ctrl_char(key) {
            match c {
                'c' => self.outcome = Some(Outcome::Abort),
                'q' => self.accept(),
                ' ' if self.focus == Focus::Filter && !self.completion.open => self.open_completion(),
                'y' | 's' => {
                    self.completion.close();
                    self.open_save_prompt();
                }
                'f' => {
                    self.completion.close();
                    self.reload_source();
                }
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Tab | KeyCode::BackTab => {
                let delta = if key.code == KeyCode::Tab { 1 } else { -1 };
                if self.completion.open {
                    self.completion.step(delta);
                } else {
                    self.set_focus(self.focus.step(delta));
                }
            }
            KeyCode::Esc => {
                self.completion.close();
                self.status_message.clear();
            }
            _ => {}
        }
    }
}

pub use completion::MAX_VISIBLE_ROWS;

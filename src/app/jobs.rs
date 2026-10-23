use super::{App, DEBOUNCE, Focus};
use crate::ansi::Document;
use crate::job::{Job, JobKind, JobResult, Reply};
use crate::jq::{Handle, Options};
use std::sync::Arc;
use std::time::Instant;

// What the cached jq results answer: the input's generation (bumped on reload), plus whether it was slurped.
pub(super) type InputKey = (u64, bool);
// ... and for a filter run, the filter and options too.
pub(super) type RunKey = (u64, String, Options);

// A jq result together with the state it answers; it only counts while the state is unchanged.
#[derive(Debug)]
pub(super) struct Cache<K, V>(Option<(K, V)>);

impl<K, V> Default for Cache<K, V> {
    fn default() -> Self {
        Self(None)
    }
}

impl<K: PartialEq, V> Cache<K, V> {
    pub(super) fn get(&self, key: &K) -> Option<&V> {
        self.0.as_ref().filter(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub(super) fn set(&mut self, key: K, value: V) {
        self.0 = Some((key, value));
    }

    pub(super) fn is_stale(&self, key: &K) -> bool {
        self.get(key).is_none()
    }
}

// A jq run handed out and not answered yet; at most one per job kind.
#[derive(Debug)]
pub(super) struct InFlight {
    generation: u64,
    kind: JobKind,
    pub(super) handle: Handle,
}

fn error_doc(stderr: &str, fallback: &str) -> Document {
    Document::plain(if stderr.trim().is_empty() { fallback } else { stderr })
}

impl App {
    pub fn working(&self) -> bool {
        self.inflight.iter().any(Option::is_some)
    }

    pub fn take_jobs(&mut self) -> Vec<Job> {
        std::mem::take(&mut self.outbox)
    }

    pub(super) fn blank_json(&self) -> bool {
        self.json.trim().is_empty()
    }

    pub(super) fn run_key(&self) -> RunKey {
        (self.generation, self.applied_filter.clone(), Options { color: true, ..self.options })
    }

    pub(super) fn input_key(&self) -> InputKey {
        (self.generation, self.options.slurp)
    }

    pub(super) fn want(&mut self, kind: JobKind) {
        let slot = kind.slot();
        match &self.inflight[slot] {
            None => {
                let handle = Handle::new(self.timeout);
                self.inflight[slot] = Some(InFlight {
                    generation: self.generation,
                    kind: kind.clone(),
                    handle: handle.clone(),
                });
                self.outbox.push(Job {
                    generation: self.generation,
                    json: Arc::clone(&self.json),
                    kind,
                    handle,
                });
            }
            // The running job answers something no longer asked: stop it; its reply frees the slot and
            // the current request goes out then.
            Some(running) if (running.generation, &running.kind) != (self.generation, &kind) => {
                running.handle.cancel();
            }
            Some(_) => {}
        }
    }

    // Requests whatever the current state needs and the caches lack.
    pub(super) fn request_missing(&mut self) {
        if self.blank_json() {
            return;
        }
        if self.validity.is_stale(&self.generation) {
            self.want(JobKind::Validate);
        }
        let slurp = self.options.slurp;
        if self.pretty.is_stale(&self.input_key()) {
            self.want(JobKind::Pretty { slurp });
        }
        if !self.applied_filter.trim().is_empty() && self.run.is_stale(&self.run_key()) {
            let options = Options { color: true, ..self.options };
            self.want(JobKind::Run { filter: self.applied_filter.clone(), options });
        }
        if self.paths.is_stale(&self.input_key()) {
            self.want(JobKind::Paths { slurp });
        }
    }

    pub(super) fn refresh(&mut self) {
        self.rebuild_json_pane();
        self.rebuild_results();
        self.request_missing();
    }

    pub(super) fn on_reply(&mut self, reply: Reply) {
        self.inflight[reply.kind.slot()] = None;
        if !reply.result.cancelled() && reply.generation == self.generation {
            match (reply.kind, reply.result) {
                (JobKind::Validate, JobResult::Output(out)) => self.validity.set(reply.generation, out),
                (JobKind::Pretty { slurp }, JobResult::Output(out)) if slurp == self.options.slurp => {
                    self.pretty.set((reply.generation, slurp), out);
                }
                (JobKind::Run { filter, options }, JobResult::Output(out)) => {
                    let key = (reply.generation, filter, options);
                    if key == self.run_key() {
                        self.run.set(key, out);
                    }
                }
                (JobKind::Paths { slurp }, JobResult::Paths(res)) if slurp == self.options.slurp => {
                    self.paths.set((reply.generation, slurp), res);
                    if std::mem::take(&mut self.completion_pending) && self.focus == Focus::Filter {
                        self.open_completion();
                    }
                }
                _ => {}
            }
        }
        self.refresh();
    }

    pub(super) fn rebuild_json_pane(&mut self) {
        if self.blank_json() {
            return self.json_pane.set(Document::default(), false);
        }
        let Some(out) = self.pretty.get(&self.input_key()) else { return };
        let (doc, error) = if out.ok() {
            (Document::new(&out.stdout, self.options.color), false)
        } else {
            (error_doc(&out.stderr, "jq error"), true)
        };
        self.json_pane.set(doc, error);
    }

    pub(super) fn rebuild_results(&mut self) {
        if self.blank_json() {
            self.json_valid = true;
            self.json_count = None;
            return self.results_pane.set(Document::default(), false);
        }
        let Some(validity) = self.validity.get(&self.generation) else { return };
        self.json_valid = validity.ok();
        self.json_count = validity.ok().then(|| validity.stdout.trim().parse().ok()).flatten();
        if !validity.ok() {
            let doc = error_doc(&validity.stderr, "invalid JSON");
            return self.results_pane.set(doc, true);
        }
        if self.applied_filter.trim().is_empty() {
            return self.results_pane.set(Document::default(), false);
        }
        let Some(out) = self.run.get(&self.run_key()) else { return };
        if out.ok() {
            let doc = Document::new(&out.stdout, self.options.color);
            self.results_pane.set(doc, false);
        } else {
            self.results_pane.set(error_doc(&out.stderr, "jq error"), true);
        }
    }

    pub(super) fn run_now(&mut self) {
        self.debounce_deadline = None;
        self.applied_filter = self.editor.text().to_string();
        self.refresh();
    }

    pub(super) fn on_filter_change(&mut self) {
        self.debounce_deadline = Some(Instant::now() + DEBOUNCE);
    }

    pub(super) fn toggle(&mut self, flip: impl FnOnce(&mut Options)) {
        let before = self.options;
        flip(&mut self.options);
        if before.slurp != self.options.slurp || before.color != self.options.color {
            self.rebuild_json_pane();
        }
        self.run_now();
    }

    pub(super) fn reload_source(&mut self) {
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
}

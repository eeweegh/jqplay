use super::{App, Focus};
use crate::ansi::strip_ansi;
use crate::editor::LineEditor;
use crate::paths::expand_tilde;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

// What a save writes: the text of the input pane or of the result pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SaveKind {
    Input,
    Output,
}

impl SaveKind {
    fn default_path(self) -> &'static str {
        match self {
            Self::Input => "input.json",
            Self::Output => "output.json",
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Self::Input => "save input to path",
            Self::Output => "save results to path",
        }
    }

    fn slot(self) -> usize {
        self as usize
    }
}

#[derive(Debug)]
pub struct Prompt {
    pub editor: LineEditor,
    pub kind: SaveKind,
    // Set once the path turned out to exist: the next Enter overwrites it.
    pub overwrite: bool,
    text: String,
}

// Without `overwrite` an existing file (or a symlink to one) is left alone and reported as AlreadyExists.
fn write_result(target: &Path, text: &str, overwrite: bool) -> std::io::Result<()> {
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if overwrite {
        return std::fs::write(target, text);
    }
    OpenOptions::new().write(true).create_new(true).open(target)?.write_all(text.as_bytes())
}

impl App {
    pub(super) fn current_results_text(&self) -> Option<String> {
        if self.blank_json() || !self.json_valid || self.editor.text().trim().is_empty() {
            return None;
        }
        let out = self.run.get(&self.run_key())?;
        out.ok().then(|| strip_ansi(&out.stdout))
    }

    pub(super) fn current_input_text(&self) -> Option<String> {
        if self.blank_json() || !self.json_valid {
            return None;
        }
        let out = self.pretty.get(&self.input_key())?;
        out.ok().then(|| strip_ansi(&out.stdout))
    }

    // The pane that has the focus decides what is saved; the filter pane saves the results.
    pub(super) fn open_save_prompt(&mut self) {
        let (kind, text) = match self.focus {
            Focus::Json => (SaveKind::Input, self.current_input_text()),
            Focus::Filter | Focus::Results => (SaveKind::Output, self.current_results_text()),
        };
        let Some(text) = text else {
            let what = if kind == SaveKind::Input { "input" } else { "results" };
            return self.status_message = format!("nothing to save (no valid {what})");
        };
        let initial = self.last_save_path[kind.slot()].as_deref().unwrap_or(kind.default_path());
        self.prompt = Some(Prompt { editor: LineEditor::new(initial), kind, overwrite: false, text });
    }

    // An existing file is only replaced after a second Enter on the same path.
    pub(super) fn confirm_save(&mut self) {
        let Some(prompt) = self.prompt.as_mut() else { return };
        let kind = prompt.kind;
        let path = prompt.editor.text().trim().to_string();
        let target = expand_tilde(&path);
        match write_result(&target, &prompt.text, prompt.overwrite) {
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => prompt.overwrite = true,
            saved => {
                self.prompt = None;
                self.status_message = match saved {
                    Ok(()) => {
                        self.last_save_path[kind.slot()] = Some(path.clone());
                        format!("saved to {path}")
                    }
                    Err(e) => format!("save failed: {e}"),
                };
            }
        }
    }
}

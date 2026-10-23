use super::App;
use crate::ansi::strip_ansi;
use crate::editor::LineEditor;
use crate::paths::expand_tilde;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

const DEFAULT_SAVE_PATH: &str = "result.json";

#[derive(Debug)]
pub struct Prompt {
    pub editor: LineEditor,
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

    pub(super) fn open_save_prompt(&mut self) {
        let Some(text) = self.current_results_text() else {
            return self.status_message = "nothing to save (no valid results)".into();
        };
        let initial = self.last_save_path.as_deref().unwrap_or(DEFAULT_SAVE_PATH);
        self.prompt = Some(Prompt { editor: LineEditor::new(initial), overwrite: false, text });
    }

    // An existing file is only replaced after a second Enter on the same path.
    pub(super) fn confirm_save(&mut self) {
        let Some(prompt) = self.prompt.as_mut() else { return };
        let path = prompt.editor.text().trim().to_string();
        let target = expand_tilde(&path);
        match write_result(&target, &prompt.text, prompt.overwrite) {
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => prompt.overwrite = true,
            saved => {
                self.prompt = None;
                self.status_message = match saved {
                    Ok(()) => {
                        self.last_save_path = Some(path.clone());
                        format!("saved to {path}")
                    }
                    Err(e) => format!("save failed: {e}"),
                };
            }
        }
    }
}

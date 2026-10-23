use super::{App, Focus, Outcome};
use crate::completion::is_token_char;
use crate::editor::LineEditor;
use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub(super) fn is_plain_char(key: &KeyEvent) -> Option<char> {
    match key.code {
        KeyCode::Char(c) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => Some(c),
        _ => None,
    }
}

pub(super) fn ctrl_char(key: &KeyEvent) -> Option<char> {
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

impl App {
    pub(super) fn set_focus(&mut self, focus: Focus) {
        self.completion.close();
        self.focus = focus;
    }

    pub(super) fn on_paste(&mut self, text: &str) {
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

    // True once after Ctrl+Z; the event loop then suspends the process.
    pub fn take_suspend(&mut self) -> bool {
        std::mem::take(&mut self.suspend)
    }

    pub(super) fn on_key(&mut self, key: &KeyEvent) {
        if ctrl_char(key) == Some('z') {
            self.suspend = true;
            return;
        }
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
                let pane =
                    if self.focus == Focus::Json { &mut self.json_pane } else { &mut self.results_pane };
                if pane.search_key(key) {
                    return;
                }
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

    pub(super) fn prompt_key(&mut self, key: &KeyEvent) {
        match key.code {
            KeyCode::Enter => self.confirm_save(),
            KeyCode::Esc => self.prompt = None,
            _ => {
                if let Some(p) = &mut self.prompt {
                    p.overwrite = false;
                    apply_edit_key(&mut p.editor, key, false);
                }
            }
        }
    }

    // Returns true when the filter line consumed the key.
    pub(super) fn filter_key(&mut self, key: &KeyEvent) -> bool {
        let open = self.completion.open;
        let newline = (key.code == KeyCode::Enter
            && (!open || key.modifiers.intersects(KeyModifiers::ALT | KeyModifiers::SHIFT)))
            || ctrl_char(key) == Some('j');
        if newline {
            self.editor.insert_newline();
            self.completion.close();
            self.on_filter_change();
            return true;
        }
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
                let current = self.editor.text();
                let recalled =
                    if up { self.history.recall_up(current) } else { self.history.recall_down(current) };
                if let Some(text) = recalled {
                    self.editor.set_text(&text);
                }
                self.on_filter_change();
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

    pub(super) fn accept(&mut self) {
        self.outcome = Some(Outcome::Clean(self.editor.text().to_string()));
    }

    // Single-key option toggles (plus `q` to accept); only reachable from the JSON and Result panes.
    pub(super) fn option_key(&mut self, key: &KeyEvent) -> bool {
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

    pub(super) fn global_key(&mut self, key: &KeyEvent) {
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

use super::App;
use crate::completion::{CANDIDATE_LIMIT, Completion, find_current_token, match_paths};

impl App {
    pub(super) fn open_completion(&mut self) {
        if self.blank_json() {
            return self.status_message = "no JSON loaded".into();
        }
        let candidates = match self.paths.get(&self.input_key()) {
            Some(Ok(c)) => c.clone(),
            Some(Err(e)) => return self.status_message = e.stderr.clone(),
            None => {
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
    pub(super) fn auto_open_completion(&mut self) {
        let Some(Ok(candidates)) = self.paths.get(&self.input_key()) else { return };
        let (start, token) = find_current_token(self.editor.text(), self.editor.cursor());
        let filtered = match_paths(candidates, &token, CANDIDATE_LIMIT);
        if !token.starts_with('.') || filtered.is_empty() || filtered == [token] {
            return;
        }
        let candidates = candidates.clone();
        self.completion = Completion { open: true, candidates, filtered, selected: 0, token_start: start };
    }

    pub(super) fn refresh_completion(&mut self) {
        let (start, token) = find_current_token(self.editor.text(), self.editor.cursor());
        let filtered = match_paths(&self.completion.candidates, &token, CANDIDATE_LIMIT);
        if filtered.is_empty() || filtered == [token] {
            return self.completion.close();
        }
        self.completion.token_start = start;
        self.completion.filtered = filtered;
        self.completion.selected = 0;
    }

    pub(super) fn accept_completion(&mut self) {
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
}

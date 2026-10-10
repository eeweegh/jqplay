// Single-line editor with the readline (Emacs) subset used by the filter pane; cursor counts chars.
#[derive(Debug, Clone, Default)]
pub struct LineEditor {
    text: String,
    cursor: usize,
    kill: String,
}

impl LineEditor {
    pub fn new(text: &str) -> Self {
        Self { text: text.to_string(), cursor: text.chars().count(), kill: String::new() }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn set_text(&mut self, text: &str) {
        self.text = text.to_string();
        self.cursor = self.len();
    }

    pub fn set_cursor(&mut self, cursor: usize) {
        self.cursor = cursor.min(self.len());
    }

    pub fn len(&self) -> usize {
        self.text.chars().count()
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    fn byte(&self, ci: usize) -> usize {
        self.text.char_indices().nth(ci).map_or(self.text.len(), |(b, _)| b)
    }

    fn chars(&self) -> Vec<char> {
        self.text.chars().collect()
    }

    pub fn insert(&mut self, s: &str) {
        let at = self.byte(self.cursor);
        self.text.insert_str(at, s);
        self.cursor += s.chars().count();
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.delete();
        }
    }

    pub fn delete(&mut self) {
        if self.cursor < self.len() {
            let at = self.byte(self.cursor);
            self.text.remove(at);
        }
    }

    pub fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.len());
    }

    pub fn home(&mut self) {
        self.cursor = 0;
    }

    pub fn end(&mut self) {
        self.cursor = self.len();
    }

    pub fn kill_to_end(&mut self) {
        let at = self.byte(self.cursor);
        self.kill = self.text.split_off(at);
    }

    pub fn kill_to_start(&mut self) {
        let at = self.byte(self.cursor);
        self.kill = self.text.drain(..at).collect();
        self.cursor = 0;
    }

    pub fn kill_word(&mut self) {
        let chars = self.chars();
        let mut start = self.cursor;
        while start > 0 && chars[start - 1].is_whitespace() {
            start -= 1;
        }
        while start > 0 && !chars[start - 1].is_whitespace() {
            start -= 1;
        }
        let (a, b) = (self.byte(start), self.byte(self.cursor));
        self.kill = self.text.drain(a..b).collect();
        self.cursor = start;
    }

    pub fn yank(&mut self) {
        let kill = self.kill.clone();
        self.insert(&kill);
    }

    pub fn transpose(&mut self) {
        let mut chars = self.chars();
        let n = chars.len();
        let (swap, new_cursor) = match self.cursor {
            c if c > 0 && c < n => (c - 1, c + 1),
            c if c == n && n >= 2 => (n - 2, c),
            _ => return,
        };
        chars.swap(swap, swap + 1);
        self.text = chars.into_iter().collect();
        self.cursor = new_cursor;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ed(text: &str, cursor: usize) -> LineEditor {
        let mut e = LineEditor::new(text);
        e.set_cursor(cursor);
        e
    }

    #[test]
    fn insert_and_delete_are_char_aware() {
        let mut e = ed("aéb", 2);
        e.insert("ü");
        assert_eq!((e.text(), e.cursor()), ("aéüb", 3));
        e.backspace();
        e.delete();
        assert_eq!(e.text(), "aé");
    }

    #[test]
    fn kill_and_yank() {
        let mut e = ed("foo bar baz", 7);
        e.kill_to_end();
        assert_eq!(e.text(), "foo bar");
        e.home();
        e.yank();
        assert_eq!(e.text(), " bazfoo bar");
        let mut e = ed("foo bar baz", 7);
        e.kill_to_start();
        assert_eq!((e.text(), e.cursor()), (" baz", 0));
        e.end();
        e.yank();
        assert_eq!(e.text(), " bazfoo bar");
    }

    #[test]
    fn kill_word_skips_trailing_space() {
        let mut e = ed("foo bar  ", 9);
        e.kill_word();
        assert_eq!((e.text(), e.cursor()), ("foo ", 4));
    }

    #[test]
    fn transpose_mid_and_end() {
        let mut e = ed("abcd", 2);
        e.transpose();
        assert_eq!((e.text(), e.cursor()), ("acbd", 3));
        let mut e = ed("abcd", 4);
        e.transpose();
        assert_eq!((e.text(), e.cursor()), ("abdc", 4));
        let mut e = ed("abcd", 0);
        e.transpose();
        assert_eq!(e.text(), "abcd");
    }
}

// Multi-line editor with the readline (Emacs) subset used by the filter pane; cursor counts chars.
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

    fn prev_char(&self) -> Option<char> {
        self.cursor.checked_sub(1).and_then(|i| self.text.chars().nth(i))
    }

    fn next_char(&self) -> Option<char> {
        self.text.chars().nth(self.cursor)
    }

    // Typed character with bracket/quote pairing: pairs on open, steps over a matching closer.
    pub fn insert_char(&mut self, c: char, autopair: bool) {
        let plain = c.encode_utf8(&mut [0; 4]).to_string();
        if !autopair {
            self.insert(&plain);
            return;
        }
        let (prev, next) = (self.prev_char(), self.next_char());
        let roomy = next.is_none_or(|n| n.is_whitespace() || matches!(n, ')' | ']' | '}'));
        match (c, closer_for(c)) {
            (')' | ']' | '}', _) if next == Some(c) => self.right(),
            ('"', _) if next == Some('"') => self.right(),
            ('"', _) if prev.is_some_and(|p| p.is_alphanumeric() || p == '_' || p == '\\') => {
                self.insert(&plain);
            }
            (_, Some(close)) if roomy => {
                self.insert(&format!("{c}{close}"));
                self.left();
            }
            _ => self.insert(&plain),
        }
    }

    // Backspace that removes an empty pair like (|) in one go.
    pub fn backspace_pair(&mut self) {
        let between_pair =
            self.prev_char().and_then(closer_for).is_some_and(|close| self.next_char() == Some(close));
        if between_pair {
            self.delete();
        }
        self.backspace();
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

    // Char index where the cursor's line starts / ends (before its '\n').
    fn line_start(&self) -> usize {
        let chars = self.chars();
        (0..self.cursor).rev().find(|&i| chars[i] == '\n').map_or(0, |i| i + 1)
    }

    fn line_end(&self) -> usize {
        let chars = self.chars();
        (self.cursor..chars.len()).find(|&i| chars[i] == '\n').unwrap_or(chars.len())
    }

    pub fn line_count(&self) -> usize {
        self.text.matches('\n').count() + 1
    }

    pub fn row_col(&self) -> (usize, usize) {
        let row = self.chars()[..self.cursor].iter().filter(|c| **c == '\n').count();
        (row, self.cursor - self.line_start())
    }

    pub fn on_first_line(&self) -> bool {
        self.row_col().0 == 0
    }

    pub fn on_last_line(&self) -> bool {
        self.row_col().0 + 1 == self.line_count()
    }

    pub fn insert_newline(&mut self) {
        self.insert("\n");
    }

    // Moves to the same column on the previous/next line (clamped); false when there is none.
    pub fn up(&mut self) -> bool {
        let start = self.line_start();
        if start == 0 {
            return false;
        }
        let col = self.cursor - start;
        let chars = self.chars();
        let prev_start = (0..start - 1).rev().find(|&i| chars[i] == '\n').map_or(0, |i| i + 1);
        self.cursor = prev_start + col.min(start - 1 - prev_start);
        true
    }

    pub fn down(&mut self) -> bool {
        let end = self.line_end();
        if end >= self.len() {
            return false;
        }
        let col = self.cursor - self.line_start();
        let chars = self.chars();
        let next_start = end + 1;
        let next_end = (next_start..chars.len()).find(|&i| chars[i] == '\n').unwrap_or(chars.len());
        self.cursor = next_start + col.min(next_end - next_start);
        true
    }

    pub fn home(&mut self) {
        self.cursor = self.line_start();
    }

    pub fn end(&mut self) {
        self.cursor = self.line_end();
    }

    pub fn kill_to_end(&mut self) {
        let (a, b) = (self.byte(self.cursor), self.byte(self.line_end()));
        self.kill = self.text.drain(a..b).collect();
    }

    pub fn kill_to_start(&mut self) {
        let start = self.line_start();
        let (a, b) = (self.byte(start), self.byte(self.cursor));
        self.kill = self.text.drain(a..b).collect();
        self.cursor = start;
    }

    pub fn kill_word(&mut self) {
        let chars = self.chars();
        let line_start = self.line_start();
        let mut start = self.cursor;
        while start > line_start && chars[start - 1].is_whitespace() {
            start -= 1;
        }
        while start > line_start && !chars[start - 1].is_whitespace() {
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

fn closer_for(open: char) -> Option<char> {
    match open {
        '(' => Some(')'),
        '[' => Some(']'),
        '{' => Some('}'),
        '"' => Some('"'),
        _ => None,
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

    fn typed(text: &str, autopair: bool) -> LineEditor {
        let mut e = LineEditor::default();
        text.chars().for_each(|c| e.insert_char(c, autopair));
        e
    }

    #[test]
    fn openers_insert_pairs_with_cursor_inside() {
        for (open, want) in [('(', "()"), ('[', "[]"), ('{', "{}"), ('"', "\"\"")] {
            let e = typed(&open.to_string(), true);
            assert_eq!((e.text(), e.cursor()), (want, 1), "{open}");
        }
    }

    #[test]
    fn no_pairing_before_a_word_or_when_disabled() {
        let mut e = ed("foo", 0);
        e.insert_char('(', true);
        assert_eq!(e.text(), "(foo");
        assert_eq!(typed("([\"", false).text(), "([\"");
    }

    #[test]
    fn closers_step_over_instead_of_duplicating() {
        assert_eq!(typed(".[0]", true).text(), ".[0]");
        assert_eq!(typed("select(.a == \"x\")", true).text(), "select(.a == \"x\")");
        assert_eq!(typed("{a: [1, (2)]}", true).text(), "{a: [1, (2)]}");
        assert_eq!(typed("{a: [1, (2)]}", true).cursor(), 13);
    }

    #[test]
    fn quote_after_word_or_backslash_is_single() {
        assert_eq!(typed("\"abc\"", true).text(), "\"abc\"");
        let mut e = ed("\"a\\", 3);
        e.insert_char('"', true);
        assert_eq!(e.text(), "\"a\\\"");
    }

    #[test]
    fn backspace_removes_empty_pair_only() {
        let mut e = typed("(", true);
        e.backspace_pair();
        assert_eq!(e.text(), "");
        let mut e = typed("[1", true);
        e.backspace_pair();
        assert_eq!((e.text(), e.cursor()), ("[]", 1));
    }

    #[test]
    fn rows_columns_and_line_counts() {
        let e = ed("ab\ncde\nf", 5);
        assert_eq!((e.row_col(), e.line_count()), ((1, 2), 3));
        assert!(!e.on_first_line() && !e.on_last_line());
        assert!(ed("x\ny", 0).on_first_line() && ed("x\ny", 3).on_last_line());
        assert_eq!(ed("", 0).line_count(), 1);
    }

    #[test]
    fn up_and_down_keep_the_column_and_clamp() {
        let mut e = ed("abcdef\nab\nabcd", 13);
        assert!(e.up());
        assert_eq!(e.cursor(), 9);
        assert!(e.up() && e.row_col() == (0, 2));
        assert!(!e.up());
        assert!(e.down() && e.down());
        assert!(!e.down());
        assert_eq!(e.row_col(), (2, 2));
    }

    #[test]
    fn home_end_and_kills_stay_on_the_line() {
        let mut e = ed("one\ntwo three\nfour", 9);
        e.home();
        assert_eq!(e.cursor(), 4);
        e.end();
        assert_eq!(e.cursor(), 13);
        e.set_cursor(7);
        e.kill_to_end();
        assert_eq!(e.text(), "one\ntwo\nfour");
        e.kill_to_start();
        assert_eq!((e.text(), e.cursor()), ("one\n\nfour", 4));
        let mut e = ed("a\nfoo bar", 9);
        e.kill_word();
        assert_eq!(e.text(), "a\nfoo ");
        e.kill_word();
        e.kill_word();
        assert_eq!(e.text(), "a\n");
    }

    #[test]
    fn newline_inserts_and_backspace_joins_lines() {
        let mut e = ed("ab", 1);
        e.insert_newline();
        assert_eq!((e.text(), e.row_col()), ("a\nb", (1, 0)));
        e.backspace();
        assert_eq!(e.text(), "ab");
    }
}

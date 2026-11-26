// Multi-line editor with the readline (Emacs) subset used by the filter pane; cursor counts chars.
// Columns in front of the text of a filter line in its pane (the "> " prompt or its blank continuation).
pub const PROMPT_WIDTH: usize = 2;

// What a double click (a word) or a triple click (a line) selects, and what dragging afterwards extends by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    Word,
    Line,
}

#[derive(Debug, Clone, Default)]
pub struct LineEditor {
    text: String,
    cursor: usize,
    kill: String,
    // The other end of the selection (the cursor is one end); none, or equal to the cursor, selects nothing.
    anchor: Option<usize>,
}

impl LineEditor {
    pub fn new(text: &str) -> Self {
        Self { text: text.to_string(), cursor: text.chars().count(), kill: String::new(), anchor: None }
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
        self.anchor = None;
    }

    pub fn set_cursor(&mut self, cursor: usize) {
        self.cursor = cursor.min(self.len());
        self.anchor = None;
    }

    // The selected range as char indices (start, end), or None when nothing is selected.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let anchor = self.anchor?.min(self.len());
        (anchor != self.cursor).then(|| (anchor.min(self.cursor), anchor.max(self.cursor)))
    }

    pub fn selected_text(&self) -> Option<String> {
        let (a, b) = self.selection()?;
        Some(self.text[self.byte(a)..self.byte(b)].to_string())
    }

    // Starts a selection at the cursor; moving the cursor with `extend_to_row_col` then grows it.
    pub fn start_selection(&mut self) {
        self.anchor = Some(self.cursor);
    }

    pub fn clear_selection(&mut self) {
        self.anchor = None;
    }

    // Removes the selected text, leaving the cursor where it started; false when nothing was selected.
    pub fn delete_selection(&mut self) -> bool {
        let Some((a, b)) = self.selection() else {
            self.anchor = None;
            return false;
        };
        let (start, end) = (self.byte(a), self.byte(b));
        self.text.drain(start..end);
        self.cursor = a;
        self.anchor = None;
        true
    }

    // Removes the selected text and keeps it as the last kill, so Ctrl+Y brings it back.
    pub fn cut_selection(&mut self) -> Option<String> {
        let text = self.selected_text()?;
        self.delete_selection();
        self.kill.clone_from(&text);
        Some(text)
    }

    // The run of same-kind characters (word, blanks or punctuation) around the cursor, within its line, as char
    // indices (start, end). At the end of a line that is the run before the cursor.
    pub fn word_range(&self) -> Option<(usize, usize)> {
        fn kind(c: char) -> u8 {
            if c.is_alphanumeric() || c == '_' {
                0
            } else if c.is_whitespace() {
                1
            } else {
                2
            }
        }
        let chars = self.chars();
        let on = |i: usize| chars.get(i).copied().filter(|c| *c != '\n');
        let at = [Some(self.cursor), self.cursor.checked_sub(1)]
            .into_iter()
            .flatten()
            .find(|&i| on(i).is_some())?;
        let k = kind(chars[at]);
        let same = |i: usize| on(i).is_some_and(|c| kind(c) == k);
        let start = (0..at).rev().take_while(|&i| same(i)).last().unwrap_or(at);
        let end = (at..chars.len()).take_while(|&i| same(i)).last().map_or(at, |i| i + 1);
        Some((start, end))
    }

    // The cursor's line without its newline, as char indices.
    fn line_range(&self) -> (usize, usize) {
        (self.line_start(), self.line_end())
    }

    fn unit_range(&self, unit: Unit) -> Option<(usize, usize)> {
        match unit {
            Unit::Word => self.word_range(),
            Unit::Line => Some(self.line_range()),
        }
    }

    // Selects the word or line around the cursor.
    pub fn select_unit(&mut self, unit: Unit) {
        if let Some((start, end)) = self.unit_range(unit) {
            self.anchor = Some(start);
            self.cursor = end;
        }
    }

    // Moves the cursor to `row` / `col` and selects whole words or lines from there back to `origin`, the char
    // range of the word or line a double or triple click started on.
    pub fn extend_units_to_row_col(&mut self, row: usize, col: usize, origin: (usize, usize), unit: Unit) {
        self.set_row_col(row, col);
        let (start, end) = self.unit_range(unit).unwrap_or((self.cursor, self.cursor));
        let (anchor, cursor) =
            if start < origin.0 { (origin.1, start) } else { (origin.0, end.max(origin.1)) };
        self.anchor = Some(anchor);
        self.cursor = cursor;
    }

    // Moves the cursor like `set_row_col` but keeps the selection anchor.
    pub fn extend_to_row_col(&mut self, row: usize, col: usize) {
        let anchor = self.anchor;
        self.set_row_col(row, col);
        self.anchor = anchor;
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
        self.delete_selection();
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
        if self.delete_selection() {
            return;
        }
        let between_pair =
            self.prev_char().and_then(closer_for).is_some_and(|close| self.next_char() == Some(close));
        if between_pair {
            self.delete();
        }
        self.backspace();
    }

    pub fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
        if self.cursor > 0 {
            self.cursor -= 1;
            self.delete();
        }
    }

    pub fn delete(&mut self) {
        if self.delete_selection() {
            return;
        }
        if self.cursor < self.len() {
            let at = self.byte(self.cursor);
            self.text.remove(at);
        }
    }

    pub fn left(&mut self) {
        self.anchor = None;
        self.cursor = self.cursor.saturating_sub(1);
    }

    pub fn right(&mut self) {
        self.anchor = None;
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

    // How far a viewport of `height` rows and `width` columns is scrolled (rows, columns) so that the cursor stays
    // in view, with one column of slack after the text start `PROMPT_WIDTH` columns from the left edge.
    pub fn scroll(&self, height: usize, width: usize) -> (usize, usize) {
        let (row, col) = self.row_col();
        ((row + 1).saturating_sub(height), (col + PROMPT_WIDTH + 1).saturating_sub(width))
    }

    // Puts the cursor on the character at `row` / `col`, clamped to the text.
    pub fn set_row_col(&mut self, row: usize, col: usize) {
        let lengths: Vec<usize> = self.text.split('\n').map(|line| line.chars().count()).collect();
        let row = row.min(lengths.len() - 1);
        let start: usize = lengths[..row].iter().map(|len| len + 1).sum();
        self.cursor = start + col.min(lengths[row]);
        self.anchor = None;
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
        self.anchor = None;
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
        self.anchor = None;
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
        self.anchor = None;
        self.cursor = self.line_start();
    }

    pub fn end(&mut self) {
        self.anchor = None;
        self.cursor = self.line_end();
    }

    pub fn kill_to_end(&mut self) {
        self.anchor = None;
        let (a, b) = (self.byte(self.cursor), self.byte(self.line_end()));
        self.kill = self.text.drain(a..b).collect();
    }

    pub fn kill_to_start(&mut self) {
        self.anchor = None;
        let start = self.line_start();
        let (a, b) = (self.byte(start), self.byte(self.cursor));
        self.kill = self.text.drain(a..b).collect();
        self.cursor = start;
    }

    pub fn kill_word(&mut self) {
        self.anchor = None;
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
        self.anchor = None;
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
    fn set_row_col_clamps_to_the_text() {
        let mut e = ed("ab\ncde\nf", 0);
        e.set_row_col(1, 2);
        assert_eq!(e.cursor(), 5);
        e.set_row_col(1, 99);
        assert_eq!(e.cursor(), 6, "past the end of a line stops at its end");
        e.set_row_col(99, 0);
        assert_eq!(e.cursor(), 7, "past the last line lands on the last line");
        e.set_row_col(0, 0);
        assert_eq!(e.cursor(), 0);
        let mut e = ed("", 0);
        e.set_row_col(3, 3);
        assert_eq!(e.cursor(), 0);
        let mut e = ed("\u{e9}\u{e9}x", 0);
        e.set_row_col(0, 2);
        assert_eq!(e.cursor(), 2, "columns are characters, not bytes");
    }

    #[test]
    fn scroll_keeps_the_cursor_in_view() {
        assert_eq!(ed("ab", 2).scroll(3, 20), (0, 0));
        assert_eq!(ed("a\nb\nc\nd", 7).scroll(2, 20), (2, 0));
        let long = "x".repeat(30);
        assert_eq!(ed(&long, 30).scroll(1, 18), (0, 15));
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

    #[test]
    fn selection_runs_either_way_and_is_empty_at_the_anchor() {
        let mut e = ed("hello world", 2);
        assert_eq!(e.selection(), None);
        e.start_selection();
        assert_eq!(e.selection(), None);
        e.extend_to_row_col(0, 7);
        assert_eq!((e.selection(), e.selected_text().as_deref()), (Some((2, 7)), Some("llo w")));
        e.extend_to_row_col(0, 0);
        assert_eq!((e.selection(), e.selected_text().as_deref()), (Some((0, 2)), Some("he")));
    }

    #[test]
    fn selection_counts_chars_and_spans_lines() {
        let mut e = ed("a\u{e9}\nb\u{fc}c", 1);
        e.start_selection();
        e.extend_to_row_col(1, 2);
        assert_eq!(e.selected_text().as_deref(), Some("\u{e9}\nb\u{fc}"));
    }

    #[test]
    fn typing_and_deleting_replace_the_selection() {
        let mut e = ed("foo bar baz", 4);
        e.start_selection();
        e.extend_to_row_col(0, 7);
        e.insert_char('x', false);
        assert_eq!((e.text(), e.cursor(), e.selection()), ("foo x baz", 5, None));
        let mut e = ed("foo bar baz", 4);
        e.start_selection();
        e.extend_to_row_col(0, 7);
        e.backspace();
        assert_eq!((e.text(), e.cursor()), ("foo  baz", 4));
        let mut e = ed("foo bar baz", 7);
        e.start_selection();
        e.extend_to_row_col(0, 4);
        e.delete();
        assert_eq!((e.text(), e.cursor()), ("foo  baz", 4));
        let mut e = ed("foo bar", 4);
        e.start_selection();
        e.extend_to_row_col(0, 7);
        e.insert("(");
        assert_eq!(e.text(), "foo (");
    }

    #[test]
    fn moving_the_cursor_ends_the_selection() {
        for step in [LineEditor::left, LineEditor::right, LineEditor::home, LineEditor::end] {
            let mut e = ed("foo bar", 3);
            e.start_selection();
            e.extend_to_row_col(0, 5);
            step(&mut e);
            assert_eq!(e.selection(), None);
        }
        let mut e = ed("foo bar", 3);
        e.start_selection();
        e.extend_to_row_col(0, 5);
        e.clear_selection();
        assert_eq!((e.selection(), e.text()), (None, "foo bar"));
    }

    #[test]
    fn select_word_takes_the_run_around_the_cursor() {
        let word = |text: &str, at: usize| {
            let mut e = ed(text, at);
            e.select_unit(Unit::Word);
            e.selected_text()
        };
        assert_eq!(word(".users | length", 3).as_deref(), Some("users"));
        assert_eq!(word(".users | length", 1).as_deref(), Some("users"));
        assert_eq!(word(".users | length", 0).as_deref(), Some("."));
        assert_eq!(word(".users | length", 6).as_deref(), Some(" "));
        assert_eq!(word(".users | length", 7).as_deref(), Some("|"));
        assert_eq!(word(".users | length", 15).as_deref(), Some("length"), "end of text: the word before");
        assert_eq!(word("a_1 \u{e9}t\u{e9}\nb", 6).as_deref(), Some("\u{e9}t\u{e9}"));
        assert_eq!(word("ab\ncd", 2).as_deref(), Some("ab"), "end of a line stays on that line");
        assert_eq!(word("ab\n\ncd", 3), None, "an empty line has nothing to select");
        assert_eq!(word("", 0), None);
    }

    #[test]
    fn extending_by_words_keeps_the_first_word_whole() {
        let mut e = ed("foo bar baz", 5);
        e.select_unit(Unit::Word);
        let origin = e.selection().unwrap();
        assert_eq!(origin, (4, 7));
        e.extend_units_to_row_col(0, 9, origin, Unit::Word);
        assert_eq!(e.selected_text().as_deref(), Some("bar baz"));
        e.extend_units_to_row_col(0, 1, origin, Unit::Word);
        assert_eq!(e.selected_text().as_deref(), Some("foo bar"));
        e.extend_units_to_row_col(0, 5, origin, Unit::Word);
        assert_eq!(e.selected_text().as_deref(), Some("bar"));
    }

    #[test]
    fn select_line_leaves_out_the_newline_and_extends_by_lines() {
        let mut e = ed("one\ntwo three\nfour", 6);
        e.select_unit(Unit::Line);
        let origin = e.selection().unwrap();
        assert_eq!(e.selected_text().as_deref(), Some("two three"));
        e.extend_units_to_row_col(2, 1, origin, Unit::Line);
        assert_eq!(e.selected_text().as_deref(), Some("two three\nfour"));
        e.extend_units_to_row_col(0, 1, origin, Unit::Line);
        assert_eq!(e.selected_text().as_deref(), Some("one\ntwo three"));
        let mut e = ed("one\n\nfour", 4);
        e.select_unit(Unit::Line);
        assert_eq!(e.selected_text(), None, "an empty line has nothing to select");
    }

    #[test]
    fn cut_removes_the_selection_and_keeps_it_for_yank() {
        let mut e = ed("foo bar baz", 4);
        e.start_selection();
        e.extend_to_row_col(0, 8);
        assert_eq!(e.cut_selection().as_deref(), Some("bar "));
        assert_eq!((e.text(), e.cursor()), ("foo baz", 4));
        e.end();
        e.yank();
        assert_eq!(e.text(), "foo bazbar ");
        assert_eq!(e.cut_selection(), None);
    }
}

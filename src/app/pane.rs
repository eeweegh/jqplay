use super::keys::is_plain_char;
use crate::ansi::{Document, Match};
use ratatui::crossterm::event::{KeyCode, KeyEvent};

const WHEEL_STEP: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchMode {
    Typing,
    Navigating,
}

#[derive(Debug)]
pub struct Search {
    pub query: String,
    pub mode: SearchMode,
    pub matches: Vec<Match>,
    pub current: usize,
}

#[derive(Debug, Default)]
pub struct Pane {
    pub doc: Document,
    pub error: bool,
    pub scroll_y: usize,
    pub scroll_x: usize,
    pub view_h: usize,
    pub view_w: usize,
    pub search: Option<Search>,
}

impl Pane {
    // Replaces the content, returning to the top only when it actually changed.
    pub(super) fn set(&mut self, doc: Document, error: bool) {
        let changed = doc.text() != self.doc.text() || error != self.error;
        if changed {
            self.scroll_y = 0;
            self.scroll_x = 0;
        }
        self.doc = doc;
        self.error = error;
        if changed {
            self.find_matches();
        }
    }

    pub(super) fn start_search(&mut self) {
        self.search =
            Some(Search { query: String::new(), mode: SearchMode::Typing, matches: Vec::new(), current: 0 });
    }

    // Recomputes the hits for the current query, selecting the first one at or below the top line.
    pub(super) fn find_matches(&mut self) {
        let Some(search) = &mut self.search else { return };
        search.matches = self.doc.find(&search.query);
        search.current = search.matches.iter().position(|m| m.line >= self.scroll_y).unwrap_or(0);
        self.reveal_current();
    }

    pub(super) fn step_match(&mut self, delta: isize) {
        let Some(search) = &mut self.search else { return };
        if !search.matches.is_empty() {
            search.current = crate::util::cycle(search.current, delta, search.matches.len());
            self.reveal_current();
        }
    }

    // Scrolls just enough to bring the selected hit into view.
    pub(super) fn reveal_current(&mut self) {
        let Some(&Match { line, col, len }) = self.search.as_ref().and_then(|s| s.matches.get(s.current))
        else {
            return;
        };
        let (h, w) = (self.view_h.max(1), self.view_w.max(1));
        if line < self.scroll_y || line >= self.scroll_y + h {
            self.scroll_y = line.saturating_sub(h / 2);
        }
        if col < self.scroll_x || col + len > self.scroll_x + w {
            self.scroll_x = if col + len <= w { 0 } else { col.saturating_sub(w / 3) };
        }
    }

    // Returns true when the key belonged to the search.
    pub(super) fn search_key(&mut self, key: &KeyEvent) -> bool {
        let mode = self.search.as_ref().map(|s| s.mode);
        let plain = is_plain_char(key);
        match (mode, key.code, plain) {
            (Some(_), KeyCode::Esc, _) => self.search = None,
            (Some(SearchMode::Typing), KeyCode::Enter, _) => {
                if let Some(s) = &mut self.search {
                    s.mode = SearchMode::Navigating;
                }
            }
            (Some(SearchMode::Typing), KeyCode::Backspace, _) => {
                if let Some(s) = &mut self.search {
                    s.query.pop();
                }
                self.find_matches();
            }
            (Some(SearchMode::Typing), _, Some(c)) => {
                if let Some(s) = &mut self.search {
                    s.query.push(c);
                }
                self.find_matches();
            }
            (Some(SearchMode::Navigating), _, Some('n')) => self.step_match(1),
            (Some(SearchMode::Navigating), _, Some('N')) => self.step_match(-1),
            (_, _, Some('/')) => self.start_search(),
            _ => return false,
        }
        true
    }

    pub(super) fn key(&mut self, code: KeyCode) {
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

    pub(super) fn wheel(&mut self, down: bool) {
        self.scroll_y = if down {
            self.scroll_y.saturating_add(WHEEL_STEP)
        } else {
            self.scroll_y.saturating_sub(WHEEL_STEP)
        };
    }
}

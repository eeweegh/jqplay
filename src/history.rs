use std::fs;
use std::path::Path;

pub const MAX_HISTORY: usize = 500;

// Strips blanks and dedupes, letting a repeated entry move to the end.
fn dedup<I: IntoIterator<Item = String>>(raw: I) -> Vec<String> {
    let mut entries: Vec<String> = Vec::new();
    for entry in raw.into_iter().map(|e| e.trim().to_string()).filter(|e| !e.is_empty()) {
        entries.retain(|e| *e != entry);
        entries.push(entry);
    }
    entries
}

// JSON array of strings; falls back to the legacy '\x1e'-joined format.
fn read_file(path: &Path) -> std::io::Result<Vec<String>> {
    let raw = fs::read_to_string(path)?;
    let loaded = serde_json::from_str::<Vec<serde_json::Value>>(&raw).map_or_else(
        |_| raw.split('\x1e').map(String::from).collect::<Vec<_>>(),
        |v| v.into_iter().filter_map(|x| x.as_str().map(String::from)).collect(),
    );
    Ok(dedup(loaded))
}

#[derive(Debug, Default)]
pub struct FilterHistory {
    entries: Vec<String>,
    cursor: usize,
    pending: String,
    contributed: Option<String>,
}

impl FilterHistory {
    pub fn load(path: &Path) -> Self {
        let entries = read_file(path).unwrap_or_default();
        Self { cursor: entries.len(), entries, ..Self::default() }
    }

    pub fn last(&self) -> Option<&str> {
        self.entries.last().map(String::as_str)
    }

    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    pub fn recall_up(&mut self, current: &str) -> Option<String> {
        if self.cursor == 0 {
            return None;
        }
        if self.cursor == self.entries.len() {
            self.pending = current.to_string();
        }
        self.cursor -= 1;
        Some(self.entries[self.cursor].clone())
    }

    pub fn recall_down(&mut self) -> Option<String> {
        if self.cursor >= self.entries.len() {
            return None;
        }
        self.cursor += 1;
        Some(self.entries.get(self.cursor).unwrap_or(&self.pending).clone())
    }

    // Skips text already sitting at the cursor so the auto-run after a recall does not reset browsing.
    pub fn record(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() || self.entries.get(self.cursor).is_some_and(|e| e == text) {
            return;
        }
        self.entries.retain(|e| e != text);
        self.entries.push(text.to_string());
        if self.entries.len() > MAX_HISTORY {
            self.entries.remove(0);
        }
        self.cursor = self.entries.len();
    }

    // One entry per session: this session's latest successful filter; I/O errors are ignored.
    pub fn save_session_entry(&mut self, path: &Path, text: &str) {
        let text = text.trim();
        if text.is_empty() || self.contributed.as_deref() == Some(text) {
            return;
        }
        if self.write_entry(path, text).is_ok() {
            self.contributed = Some(text.to_string());
        }
    }

    fn write_entry(&self, path: &Path, text: &str) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut entries = if path.exists() { read_file(path)? } else { Vec::new() };
        entries.retain(|e| Some(e.as_str()) != self.contributed.as_deref() && e != text);
        entries.push(text.to_string());
        let start = entries.len().saturating_sub(MAX_HISTORY);
        fs::write(path, serde_json::to_string_pretty(&entries[start..])?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_moves_repeats_to_end() {
        assert_eq!(dedup(["a", " b ", "", "a"].map(String::from)), ["b", "a"]);
    }

    #[test]
    fn browse_up_and_down_restores_pending() {
        let mut h = FilterHistory { entries: vec!["a".into(), "b".into()], cursor: 2, ..Default::default() };
        assert_eq!(h.recall_up("draft").as_deref(), Some("b"));
        assert_eq!(h.recall_up("b").as_deref(), Some("a"));
        assert_eq!(h.recall_up("a"), None);
        assert_eq!(h.recall_down().as_deref(), Some("b"));
        assert_eq!(h.recall_down().as_deref(), Some("draft"));
        assert_eq!(h.recall_down(), None);
    }

    #[test]
    fn record_ignores_entry_at_cursor() {
        let mut h = FilterHistory { entries: vec!["a".into(), "b".into()], cursor: 2, ..Default::default() };
        h.recall_up("");
        h.record("b");
        assert_eq!(h.cursor, 1);
        h.record("a");
        assert_eq!(h.entries(), ["b", "a"]);
    }

    #[test]
    fn session_entry_round_trips_and_replaces_itself() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/history");
        let mut h = FilterHistory::load(&path);
        h.save_session_entry(&path, ".a");
        h.save_session_entry(&path, ".ab");
        assert_eq!(FilterHistory::load(&path).entries(), [".ab"]);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("[\n  \""));
    }

    #[test]
    fn legacy_format_is_readable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("h");
        fs::write(&path, ".a\x1e.b\x1e.a").unwrap();
        assert_eq!(FilterHistory::load(&path).entries(), [".b", ".a"]);
    }
}

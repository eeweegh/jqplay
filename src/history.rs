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

// The history file as an array with one extra, empty slot at the end for the query being typed. Recalling
// an entry and editing it overwrites that element; the whole array is written back when quitting.
#[derive(Debug)]
pub struct FilterHistory {
    entries: Vec<String>,
    cursor: usize,
}

impl Default for FilterHistory {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

impl FilterHistory {
    fn new(mut entries: Vec<String>) -> Self {
        entries.push(String::new());
        Self { cursor: entries.len() - 1, entries }
    }

    pub fn load(path: &Path) -> Self {
        Self::new(read_file(path).unwrap_or_default())
    }

    pub fn entries(&self) -> &[String] {
        &self.entries
    }

    // `current` is what the filter pane holds now; it replaces the element under the cursor.
    pub fn recall_up(&mut self, current: &str) -> Option<String> {
        self.entries[self.cursor] = current.to_string();
        let target = self.cursor.checked_sub(1)?;
        self.cursor = target;
        Some(self.entries[target].clone())
    }

    pub fn recall_down(&mut self, current: &str) -> Option<String> {
        self.entries[self.cursor] = current.to_string();
        let target = self.cursor + 1;
        let entry = self.entries.get(target)?.clone();
        self.cursor = target;
        Some(entry)
    }

    // Merges the array into the file's current contents, so nothing already stored is ever lost (edited
    // entries sit next to their originals, other sessions' entries survive). `current` replaces the
    // element under the cursor and ends up last. Blanks are dropped and identical entries collapse into
    // their first occurrence; I/O errors are ignored.
    pub fn save(&mut self, path: &Path, current: &str) {
        self.entries[self.cursor] = current.to_string();
        let stored = read_file(path).unwrap_or_default();
        let mut merged: Vec<String> = Vec::new();
        for entry in stored.iter().chain(&self.entries).map(|e| e.trim()).filter(|e| !e.is_empty()) {
            if !merged.iter().any(|m| m == entry) {
                merged.push(entry.to_string());
            }
        }
        let current = current.trim();
        if !current.is_empty() {
            merged.retain(|e| e != current);
            merged.push(current.to_string());
        }
        let _ = write_entries(path, &merged);
    }
}

fn write_entries(path: &Path, entries: &[String]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let start = entries.len().saturating_sub(MAX_HISTORY);
    fs::write(path, serde_json::to_string_pretty(&entries[start..])?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dedup_moves_repeats_to_end() {
        assert_eq!(dedup(["a", " b ", "", "a"].map(String::from)), ["b", "a"]);
    }

    fn history(entries: &[&str]) -> FilterHistory {
        FilterHistory::new(entries.iter().map(ToString::to_string).collect())
    }

    #[test]
    fn starts_on_the_empty_slot_and_up_reaches_the_last_entry() {
        let mut h = history(&["a", "b"]);
        assert_eq!(h.recall_down(""), None);
        assert_eq!(h.recall_up("draft").as_deref(), Some("b"));
        assert_eq!(h.recall_up("b").as_deref(), Some("a"));
        assert_eq!(h.recall_up("a"), None);
        assert_eq!(h.recall_down("a").as_deref(), Some("b"));
        assert_eq!(h.recall_down("b").as_deref(), Some("draft"));
        assert_eq!(h.recall_down("draft"), None);
    }

    #[test]
    fn save_merges_with_the_file_so_edited_entries_never_lose_their_original() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sub/history");
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, r#"[".a", ".b", ".c"]"#).unwrap();
        let mut h = FilterHistory::load(&path);
        h.recall_up("");
        h.recall_up(".c");
        h.save(&path, ".b");
        assert_eq!(FilterHistory::load(&path).entries(), [".a", ".c", ".b", ""]);
        // Edit a recalled entry: the original stays, the edit is added.
        let mut h = FilterHistory::load(&path);
        h.recall_up("");
        h.recall_up(".b");
        h.recall_up(".c-edited");
        h.save(&path, ".a");
        assert_eq!(FilterHistory::load(&path).entries(), [".c", ".b", ".c-edited", ".a", ""]);
        // Erasing a recalled entry deletes nothing.
        let mut h = FilterHistory::load(&path);
        h.recall_up("");
        h.save(&path, "");
        assert_eq!(FilterHistory::load(&path).entries(), [".c", ".b", ".c-edited", ".a", ""]);
        // A session that started before another one saved still keeps the other's entries.
        let mut stale = FilterHistory::load(&path);
        fs::write(&path, r#"[".other"]"#).unwrap();
        stale.save(&path, ".mine");
        assert_eq!(
            FilterHistory::load(&path).entries(),
            [".other", ".c", ".b", ".c-edited", ".a", ".mine", ""]
        );
        assert!(fs::read_to_string(&path).unwrap().starts_with("[\n  \""));
    }

    #[test]
    fn legacy_format_is_readable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("h");
        fs::write(&path, ".a\x1e.b\x1e.a").unwrap();
        assert_eq!(FilterHistory::load(&path).entries(), [".b", ".a", ""]);
    }
}

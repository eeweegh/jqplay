use std::fs::{self, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

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

// Filters can embed secrets, so the file and any directory created for it are private to the user. The
// write goes through a temporary file and a rename, so a crash never leaves a truncated history behind.
fn write_entries(path: &Path, entries: &[String]) -> std::io::Result<()> {
    // Writing through a symlink (e.g. a dotfiles-managed history) must keep the link.
    let path = fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    if let Some(parent) = path.parent() {
        fs::DirBuilder::new().recursive(true).mode(0o700).create(parent)?;
    }
    let start = entries.len().saturating_sub(MAX_HISTORY);
    let json = serde_json::to_string_pretty(&entries[start..])?;
    let mut tmp = path.clone().into_os_string();
    tmp.push(format!(".{}.tmp", std::process::id()));
    let tmp = PathBuf::from(tmp);
    let written = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)
        .and_then(|mut file| file.write_all(json.as_bytes()).and_then(|()| file.sync_all()))
        .and_then(|()| fs::rename(&tmp, &path));
    if written.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    written
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
    fn history_is_private_and_leaves_no_temporary_file() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("state/history");
        FilterHistory::default().save(&path, ".a");
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
        assert_eq!(fs::read_dir(path.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn saving_through_a_symlink_keeps_the_link() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        let link = dir.path().join("link");
        fs::write(&real, r#"[".a"]"#).unwrap();
        std::os::unix::fs::symlink(&real, &link).unwrap();
        FilterHistory::load(&link).save(&link, ".b");
        assert!(fs::symlink_metadata(&link).unwrap().file_type().is_symlink());
        assert_eq!(FilterHistory::load(&real).entries(), [".a", ".b", ""]);
    }

    #[test]
    fn legacy_format_is_readable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("h");
        fs::write(&path, ".a\x1e.b\x1e.a").unwrap();
        assert_eq!(FilterHistory::load(&path).entries(), [".b", ".a", ""]);
    }
}

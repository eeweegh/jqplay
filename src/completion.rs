pub const CANDIDATE_LIMIT: usize = 50;
pub const MAX_VISIBLE_ROWS: usize = 8;

pub fn is_token_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '[' | ']' | '"')
}

// Scans back from `cursor` (char index) over path characters; the token always ends at the cursor.
pub fn find_current_token(text: &str, cursor: usize) -> (usize, String) {
    let chars: Vec<char> = text.chars().collect();
    let cursor = cursor.min(chars.len());
    let start = (0..cursor).rev().take_while(|&i| is_token_char(chars[i])).last().unwrap_or(cursor);
    (start, chars[start..cursor].iter().collect())
}

// Case-insensitive match with a leading '.' ignored; prefix matches rank ahead of substring ones.
pub fn match_paths(candidates: &[String], token: &str, limit: usize) -> Vec<String> {
    let norm = |s: &str| s.strip_prefix('.').unwrap_or(s).to_lowercase();
    let needle = norm(token);
    if needle.is_empty() {
        return candidates.iter().take(limit).cloned().collect();
    }
    let (mut prefix, mut substring) = (Vec::new(), Vec::new());
    for c in candidates {
        let hay = norm(c);
        if hay.starts_with(&needle) {
            prefix.push(c.clone());
        } else if hay.contains(&needle) {
            substring.push(c.clone());
        }
    }
    prefix.extend(substring);
    prefix.truncate(limit);
    prefix
}

#[derive(Debug, Clone, Default)]
pub struct Completion {
    pub open: bool,
    pub candidates: Vec<String>,
    pub filtered: Vec<String>,
    pub selected: usize,
    pub token_start: usize,
}

impl Completion {
    pub fn close(&mut self) {
        *self = Self::default();
    }

    pub fn step(&mut self, delta: isize) {
        let n = self.filtered.len();
        if n > 0 {
            self.selected = crate::util::cycle(self.selected, delta, n);
        }
    }

    pub fn choice(&self) -> Option<&str> {
        self.filtered.get(self.selected).map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn token_stops_at_boundary() {
        assert_eq!(find_current_token(".a | .us", 8), (5, ".us".into()));
        assert_eq!(find_current_token(".a | ", 5), (5, String::new()));
        assert_eq!(find_current_token(".a.b", 2), (0, ".a".into()));
        assert_eq!(find_current_token("", 0), (0, String::new()));
    }

    #[test]
    fn prefix_ranks_before_substring() {
        let c = v(&[".xuser", ".users", ".user.name", ".other"]);
        assert_eq!(match_paths(&c, ".US", 50), v(&[".users", ".user.name", ".xuser"]));
        assert_eq!(match_paths(&c, "", 2), v(&[".xuser", ".users"]));
        assert!(match_paths(&c, ".zzz", 50).is_empty());
    }

    #[test]
    fn selection_wraps() {
        let mut c = Completion { filtered: v(&["a", "b", "c"]), ..Completion::default() };
        c.step(-1);
        assert_eq!(c.choice(), Some("c"));
        c.step(1);
        assert_eq!(c.choice(), Some("a"));
    }
}

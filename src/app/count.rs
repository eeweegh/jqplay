// The item count of the result pane, the counterpart of the input's count in the status bar.
use crate::ansi::strip_ansi;
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResultCount {
    // JSON items, counted like the input: the length of a lone array, else the number of values.
    Items(usize),
    // Raw output is no JSON, so its lines are counted instead.
    Lines(usize),
}

impl std::fmt::Display for ResultCount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Items(n) => write!(f, "{n}"),
            Self::Lines(1) => write!(f, "1 line"),
            Self::Lines(n) => write!(f, "{n} lines"),
        }
    }
}

// `stdout` is jq's output of a successful run (colour codes allowed); None when it is not a JSON stream.
pub fn count_results(stdout: &str, raw: bool) -> Option<ResultCount> {
    let text = strip_ansi(stdout);
    if raw {
        return Some(ResultCount::Lines(text.lines().count()));
    }
    let (mut count, mut first) = (0, None);
    for value in serde_json::Deserializer::from_str(&text).into_iter::<Value>() {
        let value = value.ok()?;
        if count == 0 {
            first = Some(value);
        }
        count += 1;
    }
    Some(ResultCount::Items(match (count, first) {
        (1, Some(Value::Array(items))) => items.len(),
        (n, _) => n,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_values_and_unwraps_a_lone_array() {
        assert_eq!(count_results("", false), Some(ResultCount::Items(0)));
        assert_eq!(count_results("1\n2\n3\n", false), Some(ResultCount::Items(3)));
        assert_eq!(count_results("{\n  \"a\": 1\n}\n", false), Some(ResultCount::Items(1)));
        assert_eq!(count_results("[\n  1,\n  2\n]\n", false), Some(ResultCount::Items(2)));
        assert_eq!(count_results("[1]\n[2]\n", false), Some(ResultCount::Items(2)));
        assert_eq!(
            count_results("\x1b[0;32m\"a\"\x1b[0m\n\x1b[1;30mnull\x1b[0m\n", false),
            Some(ResultCount::Items(2))
        );
    }

    #[test]
    fn raw_output_counts_lines() {
        assert_eq!(count_results("ann\nbob smith\n", true), Some(ResultCount::Lines(2)));
        assert_eq!(count_results("", true), Some(ResultCount::Lines(0)));
        assert_eq!(ResultCount::Lines(1).to_string(), "1 line");
        assert_eq!(ResultCount::Lines(4).to_string(), "4 lines");
        assert_eq!(ResultCount::Items(4).to_string(), "4");
    }

    #[test]
    fn text_that_is_not_json_has_no_count() {
        assert_eq!(count_results("ann\n", false), None);
    }
}

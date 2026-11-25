// Turns a double-clicked value in the input pane into a jq constraint on the query.
//
// The input pane shows `jq .` output, one key per line and indented by two spaces per level, so the path
// to a line is found from the indentation alone: the nearest line above with a smaller indent opens the
// enclosing container.
use crate::ansi::Document;

// A scalar picked from the input pane. `rel` is the accessor relative to the enclosing array element
// (`.name`, `.a.b`, or `.` when the value is itself an array element), `prefix` the path to that array's
// elements (`.users[]`, `.[]`, or empty when no array encloses the value).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pick {
    pub prefix: String,
    pub rel: String,
    pub literal: String,
}

enum Seg {
    Key(String),
    Index,
}

struct Parsed {
    indent: usize,
    key: Option<String>,
    // The value text after the key, without the trailing comma.
    rest: String,
}

fn accessor(key: &str) -> String {
    let mut chars = key.chars();
    let ident = chars.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if ident { format!(".{key}") } else { format!(".{}", serde_json::to_string(key).unwrap_or_default()) }
}

fn parse_line(text: &str) -> Parsed {
    let trimmed = text.trim_start_matches(' ');
    let indent = text.len() - trimmed.len();
    let mut key = None;
    let mut rest = trimmed;
    if trimmed.starts_with('"') {
        let mut escaped = false;
        let end = trimmed.char_indices().skip(1).find(|&(_, c)| {
            let closing = c == '"' && !escaped;
            escaped = c == '\\' && !escaped;
            closing
        });
        if let Some((end, _)) = end {
            let after = &trimmed[end + 1..];
            if let (Some(tail), Ok(k)) =
                (after.strip_prefix(": "), serde_json::from_str::<String>(&trimmed[..=end]))
            {
                key = Some(k);
                rest = tail;
            }
        }
    }
    Parsed { indent, key, rest: rest.trim_end().strip_suffix(',').unwrap_or(rest.trim_end()).to_string() }
}

fn is_scalar(value: &str) -> bool {
    value.starts_with(|c: char| c == '"' || c == '-' || c.is_ascii_digit())
        || ["true", "false", "null"].contains(&value)
}

fn segment(p: &Parsed) -> Option<Seg> {
    // The root container (indent 0) is not an element of anything.
    (p.indent > 0).then(|| p.key.as_deref().map_or(Seg::Index, |k| Seg::Key(accessor(k))))
}

// The scalar on `line` of the pretty-printed `doc`, or None for containers, closing brackets and
// out-of-range lines.
pub fn pick(doc: &Document, line: usize) -> Option<Pick> {
    let clicked = parse_line(&doc.plain_line(line)?);
    if !is_scalar(&clicked.rest) {
        return None;
    }
    let mut segs: Vec<Seg> = segment(&clicked).into_iter().collect();
    let (mut indent, mut at) = (clicked.indent, line);
    while indent > 0 {
        let (j, parent) = (0..at).rev().find_map(|j| {
            let p = parse_line(&doc.plain_line(j)?);
            (p.indent < indent).then_some((j, p))
        })?;
        segs.extend(segment(&parent));
        (indent, at) = (parent.indent, j);
    }
    segs.reverse();
    let split = segs.iter().rposition(|s| matches!(s, Seg::Index)).map_or(0, |i| i + 1);
    let render = |segs: &[Seg]| -> String {
        let path: String = segs
            .iter()
            .map(|s| match s {
                Seg::Key(a) => a.as_str(),
                Seg::Index => "[]",
            })
            .collect();
        if path.starts_with('[') { format!(".{path}") } else { path }
    };
    let rel = render(&segs[split..]);
    Some(Pick {
        prefix: if split == 0 { String::new() } else { render(&segs[..split]) },
        rel: if rel.is_empty() { ".".into() } else { rel },
        literal: clicked.rest,
    })
}

// A code character of a jq program with the parenthesis depth around it.
struct Code {
    at: usize,
    ch: char,
    depth: usize,
}

// The characters of `s` that are jq code: string contents (but not `\(...)` interpolations) and comments are
// left out, the quotes and the interpolation parentheses too.
fn code_chars(s: &str) -> Vec<Code> {
    #[derive(PartialEq)]
    enum Frame {
        Str,
        Interp,
        Paren,
    }
    let (mut stack, mut out) = (Vec::new(), Vec::new());
    let mut chars = s.char_indices().peekable();
    while let Some((at, ch)) = chars.next() {
        let depth = stack.iter().filter(|f| **f != Frame::Str).count();
        if stack.last() == Some(&Frame::Str) {
            match ch {
                '\\' if chars.peek().is_some_and(|&(_, n)| n == '(') => {
                    chars.next();
                    stack.push(Frame::Interp);
                }
                '\\' => {
                    chars.next();
                }
                '"' => {
                    stack.pop();
                }
                _ => {}
            }
            continue;
        }
        match ch {
            '"' => stack.push(Frame::Str),
            '(' => {
                out.push(Code { at, ch, depth });
                stack.push(Frame::Paren);
            }
            ')' => {
                if stack.pop() != Some(Frame::Interp) {
                    out.push(Code { at, ch, depth: depth.saturating_sub(1) });
                }
            }
            '#' => while chars.next_if(|&(_, n)| n != '\n').is_some() {},
            _ => out.push(Code { at, ch, depth }),
        }
    }
    out
}

fn is_word_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '$' | '.')
}

// Whether `x` binds looser than `and`, so it needs parentheses to be and-ed.
fn needs_parens(x: &str) -> bool {
    let code = code_chars(x);
    code.iter().enumerate().any(|(i, c)| {
        let prev = i.checked_sub(1).map(|p| &code[p]);
        let next = code.get(i + 1);
        let adjacent = |o: Option<&Code>| o.filter(|o| o.at.abs_diff(c.at) == 1).map(|o| o.ch);
        c.depth == 0
            && match c.ch {
                '|' | ',' => true,
                '/' => adjacent(next) == Some('/'),
                '=' => adjacent(next) != Some('=') && !matches!(adjacent(prev), Some('=' | '!' | '<' | '>')),
                'o' => {
                    adjacent(next) == Some('r')
                        && !adjacent(prev).is_some_and(is_word_char)
                        && !x[c.at + 2..].starts_with(is_word_char)
                }
                _ => false,
            }
    })
}

// When the query's last pipe stage is `select(X)`: where that stage starts, and X.
fn last_select(query: &str) -> Option<(usize, &str)> {
    let code = code_chars(query);
    let last = code.last().filter(|c| c.ch == ')' && c.depth == 0 && c.at + 1 == query.len())?;
    let mut opens = Vec::new();
    let mut open = None;
    for c in &code {
        match c.ch {
            '(' => opens.push(c.at),
            ')' => open = opens.pop(),
            _ => {}
        }
    }
    let open = open?;
    let head = query[..open].trim_end().strip_suffix("select")?;
    let stage = head.trim_end();
    (!head.ends_with(is_word_char) && (stage.is_empty() || stage.ends_with('|')))
        .then(|| (head.len(), query[open + 1..last.at].trim()))
}

// The query with `pick` added as a constraint: a trailing `select(X)` is extended with `and`, anything else
// gets a `| select(...)` stage (an empty query selects from the elements of the array around the value).
pub fn add_constraint(query: &str, pick: &Pick) -> String {
    let cond = format!("{} == {}", pick.rel, pick.literal);
    let query = query.trim();
    if query.is_empty() {
        return if pick.prefix.is_empty() {
            format!("select({cond})")
        } else {
            format!("{} | select({cond})", pick.prefix)
        };
    }
    match last_select(query) {
        Some((start, x)) if needs_parens(x) => format!("{}select(({x}) and {cond})", &query[..start]),
        Some((start, x)) => format!("{}select({x} and {cond})", &query[..start]),
        None => format!("{query} | select({cond})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRETTY: &str = r#"{
  "users": [
    {
      "name": "bob",
      "tags": [
        "a",
        "b"
      ],
      "odd key": 1,
      "nested": {
        "ok": true
      }
    }
  ],
  "count": 2
}"#;

    fn picked(text: &str, needle: &str) -> Option<Pick> {
        let doc = Document::plain(text);
        let line = text.lines().position(|l| l.contains(needle)).unwrap();
        pick(&doc, line)
    }

    fn pk(prefix: &str, rel: &str, literal: &str) -> Pick {
        Pick { prefix: prefix.into(), rel: rel.into(), literal: literal.into() }
    }

    #[test]
    fn a_key_inside_an_array_element_is_relative_to_that_element() {
        assert_eq!(picked(PRETTY, "\"name\""), Some(pk(".users[]", ".name", "\"bob\"")));
        assert_eq!(picked(PRETTY, "\"ok\""), Some(pk(".users[]", ".nested.ok", "true")));
    }

    #[test]
    fn odd_keys_are_quoted() {
        assert_eq!(picked(PRETTY, "odd key"), Some(pk(".users[]", ".\"odd key\"", "1")));
    }

    #[test]
    fn a_scalar_array_element_is_the_value_itself() {
        assert_eq!(picked(PRETTY, "\"b\""), Some(pk(".users[].tags[]", ".", "\"b\"")));
    }

    #[test]
    fn a_key_outside_any_array_has_no_prefix() {
        assert_eq!(picked(PRETTY, "\"count\""), Some(pk("", ".count", "2")));
    }

    #[test]
    fn root_arrays_and_scalars() {
        assert_eq!(picked("[\n  {\n    \"a\": null\n  }\n]", "\"a\""), Some(pk(".[]", ".a", "null")));
        assert_eq!(picked("[\n  1,\n  2\n]", "2"), Some(pk(".[]", ".", "2")));
        assert_eq!(picked("5", "5"), Some(pk("", ".", "5")));
    }

    #[test]
    fn containers_and_closing_lines_are_not_values() {
        assert_eq!(picked(PRETTY, "\"users\""), None);
        assert_eq!(picked(PRETTY, "\"tags\""), None);
        assert_eq!(picked(PRETTY, "  ],"), None);
        assert_eq!(pick(&Document::plain(PRETTY), 99), None);
    }

    #[test]
    fn keys_with_escapes_and_colons_are_read_whole() {
        let text = "{\n  \"a\\\": b\": \"x: y\"\n}";
        assert_eq!(picked(text, "x: y"), Some(pk("", ".\"a\\\": b\"", "\"x: y\"")));
    }

    #[test]
    fn an_empty_query_selects_from_the_array_elements() {
        assert_eq!(
            add_constraint("", &pk(".users[]", ".name", "\"bob\"")),
            ".users[] | select(.name == \"bob\")"
        );
        assert_eq!(add_constraint("  ", &pk("", ".count", "2")), "select(.count == 2)");
    }

    #[test]
    fn a_plain_query_gets_a_select_stage() {
        assert_eq!(
            add_constraint(".users[]", &pk(".users[]", ".name", "\"bob\"")),
            ".users[] | select(.name == \"bob\")"
        );
    }

    #[test]
    fn a_trailing_select_is_extended_with_and() {
        let p = pk(".[]", ".b", "2");
        assert_eq!(add_constraint(".[] | select(.a == 1)", &p), ".[] | select(.a == 1 and .b == 2)");
        assert_eq!(add_constraint("select(.a == 1)", &p), "select(.a == 1 and .b == 2)");
        assert_eq!(
            add_constraint(".[] | select(.a == 1) | .x", &p),
            ".[] | select(.a == 1) | .x | select(.b == 2)"
        );
    }

    #[test]
    fn a_select_that_binds_looser_than_and_is_parenthesised() {
        let p = pk(".[]", ".b", "2");
        assert_eq!(add_constraint("select(.a or .c)", &p), "select((.a or .c) and .b == 2)");
        assert_eq!(add_constraint("select(.a // .c)", &p), "select((.a // .c) and .b == 2)");
        assert_eq!(add_constraint("select(.a | f)", &p), "select((.a | f) and .b == 2)");
        assert_eq!(
            add_constraint("select(.a != 1 and .c >= 3 and .d == 4)", &p),
            "select(.a != 1 and .c >= 3 and .d == 4 and .b == 2)"
        );
        assert_eq!(add_constraint("select(.color == \"or\")", &p), "select(.color == \"or\" and .b == 2)");
    }

    #[test]
    fn parentheses_in_strings_and_nested_calls_do_not_confuse_the_scan() {
        let p = pk(".[]", ".b", "2");
        assert_eq!(add_constraint("select(.a == \")\")", &p), "select(.a == \")\" and .b == 2)");
        assert_eq!(
            add_constraint("select(.a | test(\"x(\"))", &p),
            "select((.a | test(\"x(\")) and .b == 2)"
        );
        assert_eq!(add_constraint("select(.a == \"\\(.x)\")", &p), "select(.a == \"\\(.x)\" and .b == 2)");
        assert_eq!(add_constraint("map(select(.a))", &p), "map(select(.a)) | select(.b == 2)");
        assert_eq!(add_constraint("x + select(.a)", &p), "x + select(.a) | select(.b == 2)");
        assert_eq!(add_constraint("myselect(.a)", &p), "myselect(.a) | select(.b == 2)");
    }
}

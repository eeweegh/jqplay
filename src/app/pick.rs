// Turns a double-clicked value in the input pane into a jq constraint on the query.
//
// The input pane shows `jq .` output, one key per line and indented by two spaces per level, so the path
// to a line is found from the indentation alone: the nearest line above with a smaller indent opens the
// enclosing container.
use crate::ansi::Document;

// What a double-click on a scalar of the input pane asks for, with full key paths from the record root:
// the value at a path, or, for a value inside an array, that some element of the array satisfies the inner term
// (array indices are dropped, so it reads "any element ...").
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Term {
    // `path` is `.a.b`, or `.` for the record itself (or the element itself inside an array).
    Eq { path: String, literal: String },
    // `array` is the path to the elements, e.g. `.orders[]`.
    Any { array: String, inner: Box<Term> },
}

impl Term {
    fn render(&self) -> String {
        match self {
            Self::Eq { path, literal } => format!("{path} == {literal}"),
            Self::Any { array, inner } => format!("any({array}; {})", inner.render()),
        }
    }
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
pub fn pick(doc: &Document, line: usize) -> Option<Term> {
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
    Some(term(&segs, clicked.rest))
}

fn keys(segs: &[Seg]) -> String {
    segs.iter()
        .map(|s| match s {
            Seg::Key(a) => a.as_str(),
            Seg::Index => "[]",
        })
        .collect()
}

// Splits the path at each array level: `.a[].b[].c` becomes any(.a[]; any(.b[]; .c == v)).
fn term(segs: &[Seg], literal: String) -> Term {
    match segs.iter().position(|s| matches!(s, Seg::Index)) {
        None => Term::Eq { path: if segs.is_empty() { ".".into() } else { keys(segs) }, literal },
        Some(i) => {
            let base = keys(&segs[..i]);
            let array = format!("{}[]", if base.is_empty() { "." } else { &base });
            Term::Any { array, inner: Box::new(term(&segs[i + 1..], literal)) }
        }
    }
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

// The index of the `)` that closes the `(` at byte `open`, among the code characters of one text.
fn matching_close(code: &[Code], open: usize) -> Option<usize> {
    let mut opens = Vec::new();
    for c in code {
        if c.ch == '(' {
            opens.push(c.at);
        } else if c.ch == ')' && opens.pop() == Some(open) {
            return Some(c.at);
        }
    }
    None
}

// The text up to the first top-level pipe (`|=` is an update operator, not a pipe).
fn first_stage(query: &str) -> &str {
    let code = code_chars(query);
    let pipe = code.iter().enumerate().find(|(i, c)| {
        let assign = code.get(i + 1).is_some_and(|n| n.at == c.at + 1 && n.ch == '=');
        c.ch == '|' && c.depth == 0 && !assign
    });
    query[..pipe.map_or(query.len(), |(_, c)| c.at)].trim_end()
}

// When `stage` is exactly `select(X)`: X.
fn select_arg(stage: &str) -> Option<&str> {
    let after = stage.strip_prefix("select")?;
    let open = stage.len() - after.trim_start().len();
    after.trim_start().starts_with('(').then_some(())?;
    (matching_close(&code_chars(stage), open)? + 1 == stage.len())
        .then(|| stage[open + 1..stage.len() - 1].trim())
}

// The pieces of `x` between its top-level `and` words (`x` binds no looser than `and`).
fn conjuncts(x: &str) -> Vec<&str> {
    let code = code_chars(x);
    let is_and = |i: usize| {
        let at = code[i].at;
        let word =
            |k: usize| code.get(i + k).is_some_and(|c| c.at == at + k && c.ch == char::from(b"and"[k]));
        code[i].depth == 0
            && word(0)
            && word(1)
            && word(2)
            && !x[..at].ends_with(is_word_char)
            && !x[at + 3..].starts_with(is_word_char)
    };
    let (mut pieces, mut from) = (Vec::new(), 0);
    for i in 0..code.len() {
        if is_and(i) {
            pieces.push(x[from..code[i].at].trim());
            from = code[i].at + 3;
        }
    }
    pieces.push(x[from..].trim());
    pieces
}

// `any(<array>; <inner>)` -> (array, inner).
fn any_parts(piece: &str) -> Option<(&str, &str)> {
    let body = piece.strip_prefix("any(")?.strip_suffix(')')?;
    // The `(` after `any` must be closed by that last `)`, not earlier (`any(a; b) and any(c; d)`).
    (matching_close(&code_chars(piece), 3)? + 1 == piece.len()).then_some(())?;
    let semi = code_chars(body).iter().find(|c| c.ch == ';' && c.depth == 0)?.at;
    Some((body[..semi].trim(), body[semi + 1..].trim()))
}

// `x and term`, folding `term` into the `any(...)` over the same array when there is one, so that both
// conditions hold for the same element.
fn and_term(x: &str, term: &Term) -> String {
    if x.is_empty() {
        return term.render();
    }
    if needs_parens(x) {
        return format!("({x}) and {}", term.render());
    }
    if let Term::Any { array, inner } = term {
        let pieces = conjuncts(x);
        let target = pieces.iter().position(|p| any_parts(p).is_some_and(|(a, _)| a == array));
        if let Some(i) = target {
            let (_, y) = any_parts(pieces[i]).unwrap_or_default();
            let merged = format!("any({array}; {})", and_term(y, inner));
            let joined: Vec<String> = pieces
                .iter()
                .enumerate()
                .map(|(k, p)| if k == i { merged.clone() } else { (*p).to_string() })
                .collect();
            return joined.join(" and ");
        }
    }
    format!("{x} and {}", term.render())
}

// Adds `term` to a query that sees one record: a select that is the first stage takes it as an extra `and`
// condition, otherwise a new select goes in front.
fn constrain(query: &str, term: &Term) -> String {
    if query.is_empty() {
        return format!("select({})", term.render());
    }
    let stage = first_stage(query);
    match select_arg(stage) {
        Some(x) => format!("select({}){}", and_term(x, term), &query[stage.len()..]),
        None => format!("select({}) | {query}", term.render()),
    }
}

// The query without a leading `.[] |`.
fn without_iteration(query: &str) -> &str {
    let stage = first_stage(query);
    if stage != ".[]" {
        return query;
    }
    query[stage.len()..].trim_start().strip_prefix('|').map_or("", str::trim_start)
}

// The query with `term` added as a constraint that filters the input before anything else touches it. When
// the input is an array (a slurped stream), its elements are the records: a leading `.[] |` is kept or added
// and everything after it is treated as for a stream of records.
pub fn add_constraint(query: &str, term: &Term) -> String {
    let query = query.trim();
    match term {
        Term::Any { array, inner } if array == ".[]" => {
            format!(".[] | {}", constrain(without_iteration(query), inner))
        }
        _ => constrain(query, term),
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

    const RECORDS: &str = r#"{
  "name": "ann",
  "age": 31,
  "addr": {
    "city": "Gent"
  }
}
{
  "name": "bob",
  "age": 27,
  "addr": {
    "city": "Brugge"
  }
}"#;

    // The constraint for the first line containing `needle`, as jq text.
    fn picked(text: &str, needle: &str) -> Option<String> {
        term_at(text, needle).map(|t| t.render())
    }

    fn term_at(text: &str, needle: &str) -> Option<Term> {
        let doc = Document::plain(text);
        let line = text.lines().position(|l| l.contains(needle)).unwrap();
        pick(&doc, line)
    }

    fn eq(path: &str, literal: &str) -> Term {
        Term::Eq { path: path.into(), literal: literal.into() }
    }

    fn any(array: &str, inner: Term) -> Term {
        Term::Any { array: array.into(), inner: Box::new(inner) }
    }

    fn add(query: &str, term: &Term) -> String {
        add_constraint(query, term)
    }

    #[test]
    fn keys_are_full_paths_from_the_record_root() {
        assert_eq!(picked(RECORDS, "Gent").as_deref(), Some(".addr.city == \"Gent\""));
        assert_eq!(picked(RECORDS, "\"age\": 27").as_deref(), Some(".age == 27"));
    }

    #[test]
    fn each_record_of_a_stream_is_its_own_root() {
        let second = RECORDS.lines().position(|l| l.contains("Brugge")).unwrap();
        let doc = Document::plain(RECORDS);
        assert_eq!(pick(&doc, second).unwrap(), eq(".addr.city", "\"Brugge\""));
        assert_eq!(pick(&doc, 1).unwrap(), eq(".name", "\"ann\""));
    }

    #[test]
    fn odd_keys_are_quoted() {
        let t = term_at("{\n  \"odd key\": 1\n}", "odd").unwrap();
        assert_eq!(t, eq(".\"odd key\"", "1"));
    }

    #[test]
    fn values_inside_arrays_become_any() {
        assert_eq!(picked(PRETTY, "\"name\"").as_deref(), Some("any(.users[]; .name == \"bob\")"));
        assert_eq!(picked(PRETTY, "\"ok\"").as_deref(), Some("any(.users[]; .nested.ok == true)"));
        assert_eq!(picked(PRETTY, "\"odd key\"").as_deref(), Some("any(.users[]; .\"odd key\" == 1)"));
    }

    #[test]
    fn scalar_array_elements_and_nested_arrays() {
        assert_eq!(picked(PRETTY, "\"b\"").as_deref(), Some("any(.users[]; any(.tags[]; . == \"b\"))"));
        assert_eq!(
            picked("{\n  \"tags\": [\n    \"a\"\n  ]\n}", "\"a\"").as_deref(),
            Some("any(.tags[]; . == \"a\")")
        );
        assert_eq!(picked(PRETTY, "\"count\"").as_deref(), Some(".count == 2"));
    }

    #[test]
    fn bare_scalars_and_root_arrays() {
        assert_eq!(picked("5", "5").as_deref(), Some(". == 5"));
        assert_eq!(picked("[\n  1,\n  2\n]", "2").as_deref(), Some("any(.[]; . == 2)"));
        assert_eq!(
            picked("[\n  {\n    \"a\": null\n  }\n]", "\"a\"").as_deref(),
            Some("any(.[]; .a == null)")
        );
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
        assert_eq!(term_at(text, "x: y").unwrap(), eq(".\"a\\\": b\"", "\"x: y\""));
    }

    #[test]
    fn an_empty_query_becomes_a_select() {
        assert_eq!(add("", &eq(".addr.city", "\"Brugge\"")), "select(.addr.city == \"Brugge\")");
        assert_eq!(add("  ", &eq(".age", "27")), "select(.age == 27)");
    }

    #[test]
    fn a_new_select_goes_in_front_of_the_query() {
        let age = eq(".age", "27");
        assert_eq!(add(".name", &age), "select(.age == 27) | .name");
        assert_eq!(add(".users[] | .name", &age), "select(.age == 27) | .users[] | .name");
        assert_eq!(
            add(".name | select(. != \"x\")", &age),
            "select(.age == 27) | .name | select(. != \"x\")"
        );
    }

    #[test]
    fn a_first_select_takes_an_extra_and() {
        let age = eq(".age", "27");
        assert_eq!(
            add("select(.addr.city == \"Brugge\") | .name", &age),
            "select(.addr.city == \"Brugge\" and .age == 27) | .name"
        );
        assert_eq!(add("select(.a == 1)", &age), "select(.a == 1 and .age == 27)");
        assert_eq!(add("select (.a == 1) | .x", &age), "select(.a == 1 and .age == 27) | .x");
        assert_eq!(add("select(.a) | select(.b) | .x", &age), "select(.a and .age == 27) | select(.b) | .x");
    }

    #[test]
    fn a_select_that_binds_looser_than_and_is_parenthesised() {
        let age = eq(".age", "27");
        assert_eq!(add("select(.a or .b) | .name", &age), "select((.a or .b) and .age == 27) | .name");
        assert_eq!(add("select(.a // .c)", &age), "select((.a // .c) and .age == 27)");
        assert_eq!(add("select(.a | f)", &age), "select((.a | f) and .age == 27)");
        assert_eq!(
            add("select(.a != 1 and .c >= 3 and .d == 4)", &age),
            "select(.a != 1 and .c >= 3 and .d == 4 and .age == 27)"
        );
        assert_eq!(add("select(.color == \"or\")", &age), "select(.color == \"or\" and .age == 27)");
    }

    #[test]
    fn strings_parentheses_and_update_operators_do_not_confuse_the_scan() {
        let age = eq(".age", "27");
        assert_eq!(add("select(.a == \")\")", &age), "select(.a == \")\" and .age == 27)");
        assert_eq!(add("select(.a == \"x and y\")", &age), "select(.a == \"x and y\" and .age == 27)");
        assert_eq!(add("select(.a | test(\"x(\"))", &age), "select((.a | test(\"x(\")) and .age == 27)");
        assert_eq!(add("select(.a == \"\\(.x)\")", &age), "select(.a == \"\\(.x)\" and .age == 27)");
        assert_eq!(add("select(.a) |= 1", &age), "select(.age == 27) | select(.a) |= 1");
        assert_eq!(add(".a |= 1 | .b", &age), "select(.age == 27) | .a |= 1 | .b");
        assert_eq!(add("myselect(.a)", &age), "select(.age == 27) | myselect(.a)");
        assert_eq!(add("map(select(.a))", &age), "select(.age == 27) | map(select(.a))");
        assert_eq!(add("select(.a) + 1", &age), "select(.age == 27) | select(.a) + 1");
    }

    #[test]
    fn a_condition_on_the_same_array_is_merged_into_its_any() {
        let id = any(".orders[]", eq(".id", "2"));
        assert_eq!(
            add("select(any(.orders[]; .total == 9)) | .id", &id),
            "select(any(.orders[]; .total == 9 and .id == 2)) | .id"
        );
        assert_eq!(
            add("select(.name == \"x\" and any(.orders[]; .total == 9))", &id),
            "select(.name == \"x\" and any(.orders[]; .total == 9 and .id == 2))"
        );
        let d = any(".a[]", any(".b[]", eq(".d", "2")));
        assert_eq!(
            add("select(any(.a[]; any(.b[]; .c == 1)))", &d),
            "select(any(.a[]; any(.b[]; .c == 1 and .d == 2)))"
        );
    }

    #[test]
    fn a_condition_on_another_array_or_a_looser_select_is_and_ed_on() {
        let tag = any(".tags[]", eq(".", "\"b\""));
        assert_eq!(
            add("select(any(.orders[]; .total == 9))", &tag),
            "select(any(.orders[]; .total == 9) and any(.tags[]; . == \"b\"))"
        );
        let id = any(".orders[]", eq(".id", "2"));
        assert_eq!(
            add("select(.a or any(.orders[]; .total == 9))", &id),
            "select((.a or any(.orders[]; .total == 9)) and any(.orders[]; .id == 2))"
        );
        assert_eq!(
            add("select(any(.orders[]; .total == 9) and any(.tags[]; . == \"b\"))", &id),
            "select(any(.orders[]; .total == 9 and .id == 2) and any(.tags[]; . == \"b\"))"
        );
        assert_eq!(
            add("select(any(.x[]; .a) and any(.orders[]; .b))", &eq(".c", "1")),
            "select(any(.x[]; .a) and any(.orders[]; .b) and .c == 1)"
        );
    }

    #[test]
    fn the_elements_of_a_root_array_are_the_records() {
        let age = any(".[]", eq(".age", "27"));
        assert_eq!(add("", &age), ".[] | select(.age == 27)");
        assert_eq!(add(".[]", &age), ".[] | select(.age == 27)");
        assert_eq!(add(".[] | .name", &age), ".[] | select(.age == 27) | .name");
        assert_eq!(
            add(".[] | select(.name == \"bob\") | .name", &age),
            ".[] | select(.name == \"bob\" and .age == 27) | .name"
        );
        assert_eq!(
            add(".[] | .name | select(. != \"x\")", &age),
            ".[] | select(.age == 27) | .name | select(. != \"x\")"
        );
        assert_eq!(add(".[] |= 1", &age), ".[] | select(.age == 27) | .[] |= 1");
        // A query for the whole array is prefixed as well.
        assert_eq!(add("map(.name)", &age), ".[] | select(.age == 27) | map(.name)");
    }

    #[test]
    fn root_array_elements_can_be_scalars_or_hold_arrays() {
        assert_eq!(add("", &any(".[]", eq(".", "2"))), ".[] | select(. == 2)");
        let id = any(".[]", any(".orders[]", eq(".id", "2")));
        assert_eq!(
            add(".[] | select(any(.orders[]; .total == 9)) | .id", &id),
            ".[] | select(any(.orders[]; .total == 9 and .id == 2)) | .id"
        );
        assert_eq!(add("", &id), ".[] | select(any(.orders[]; .id == 2))");
    }
}

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use std::ops::Range;

// Splits `s` at the first complete SGR escape: (text before, params, text after).
fn split_sgr(s: &str) -> (&str, Option<&str>, &str) {
    let mut from = 0;
    while let Some(i) = s[from..].find("\x1b[") {
        let start = from + i;
        let body = &s[start + 2..];
        let len = body.bytes().take_while(|b| b.is_ascii_digit() || *b == b';').count();
        if body.as_bytes().get(len) == Some(&b'm') {
            return (&s[..start], Some(&body[..len]), &body[len + 1..]);
        }
        from = start + 2;
    }
    (s, None, "")
}

pub fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    loop {
        let (plain, esc, tail) = split_sgr(rest);
        out.push_str(plain);
        if esc.is_none() {
            return out;
        }
        rest = tail;
    }
}

fn basic_color(n: u16) -> Option<Color> {
    Some(match n % 10 {
        0 => Color::Black,
        1 => Color::Red,
        2 => Color::Green,
        3 => Color::Yellow,
        4 => Color::Blue,
        5 => Color::Magenta,
        6 => Color::Cyan,
        7 => Color::Gray,
        _ => return None,
    })
    .map(|c| if n >= 90 { bright(c) } else { c })
}

fn bright(c: Color) -> Color {
    match c {
        Color::Black => Color::DarkGray,
        Color::Red => Color::LightRed,
        Color::Green => Color::LightGreen,
        Color::Yellow => Color::LightYellow,
        Color::Blue => Color::LightBlue,
        Color::Magenta => Color::LightMagenta,
        Color::Cyan => Color::LightCyan,
        _ => Color::White,
    }
}

// Advances a style by one escape's worth of ';'-separated SGR parameters; an empty parameter means 0.
pub fn apply_sgr(style: Style, params: &str) -> Style {
    let p: Vec<u16> = params.split(';').map(|s| s.parse().unwrap_or(0)).collect();
    let mut style = style;
    let mut i = 0;
    while i < p.len() {
        match p[i] {
            0 => style = Style::default(),
            1 => style = style.add_modifier(Modifier::BOLD),
            2 => style = style.add_modifier(Modifier::DIM),
            3 => style = style.add_modifier(Modifier::ITALIC),
            4 => style = style.add_modifier(Modifier::UNDERLINED),
            5 => style = style.add_modifier(Modifier::SLOW_BLINK),
            7 => style = style.add_modifier(Modifier::REVERSED),
            9 => style = style.add_modifier(Modifier::CROSSED_OUT),
            21 => style = style.remove_modifier(Modifier::BOLD),
            22 => style = style.remove_modifier(Modifier::BOLD | Modifier::DIM),
            23 => style = style.remove_modifier(Modifier::ITALIC),
            24 => style = style.remove_modifier(Modifier::UNDERLINED),
            25 => style = style.remove_modifier(Modifier::SLOW_BLINK),
            27 => style = style.remove_modifier(Modifier::REVERSED),
            29 => style = style.remove_modifier(Modifier::CROSSED_OUT),
            39 => style.fg = None,
            49 => style.bg = None,
            n @ (38 | 48) => {
                let color = match (p.get(i + 1), p.get(i + 2), p.get(i + 3), p.get(i + 4)) {
                    (Some(5), Some(&c), ..) => {
                        i += 2;
                        Some(Color::Indexed(c as u8))
                    }
                    (Some(2), Some(&r), Some(&g), Some(&b)) => {
                        i += 4;
                        Some(Color::Rgb(r as u8, g as u8, b as u8))
                    }
                    _ => None,
                };
                if let Some(c) = color {
                    if n == 38 { style.fg = Some(c) } else { style.bg = Some(c) }
                }
            }
            n @ (30..=37 | 90..=97) => style.fg = basic_color(n),
            n @ (40..=47 | 100..=107) => style.bg = basic_color(n - 10),
            _ => {}
        }
        i += 1;
    }
    style
}

fn visible_width(line: &str, color: bool) -> usize {
    if !color {
        return line.chars().count();
    }
    let (mut rest, mut width) = (line, 0);
    loop {
        let (plain, esc, tail) = split_sgr(rest);
        width += plain.chars().count();
        if esc.is_none() {
            return width;
        }
        rest = tail;
    }
}

fn push_window(
    spans: &mut Vec<Span<'static>>,
    text: &str,
    style: Style,
    col: &mut usize,
    left: usize,
    end: usize,
) {
    let n = text.chars().count();
    let (from, to) = (left.saturating_sub(*col).min(n), end.saturating_sub(*col).min(n));
    *col += n;
    if from < to {
        spans.push(Span::styled(text.chars().skip(from).take(to - from).collect::<String>(), style));
    }
}

// Text of a pane with line boundaries and per-line SGR state indexed up front, so a frame only
// parses the lines (and columns) that are actually visible.
#[derive(Debug, Clone, Default)]
pub struct Document {
    text: String,
    lines: Vec<Range<usize>>,
    states: Vec<Style>,
    color: bool,
    width: usize,
}

impl Document {
    pub fn new(raw: &str, color: bool) -> Self {
        let text = if color {
            raw.trim_end_matches('\n').to_string()
        } else {
            strip_ansi(raw).trim_end_matches('\n').to_string()
        };
        let mut lines = Vec::new();
        let mut states = Vec::new();
        let (mut state, mut width, mut pos) = (Style::default(), 0, 0);
        if !text.is_empty() {
            for line in text.split('\n') {
                lines.push(pos..pos + line.len());
                pos += line.len() + 1;
                width = width.max(visible_width(line, color));
                if color {
                    states.push(state);
                    let mut rest = line;
                    while let (_, Some(p), tail) = split_sgr(rest) {
                        state = apply_sgr(state, p);
                        rest = tail;
                    }
                }
            }
        }
        Self { text, lines, states, color, width }
    }

    pub fn plain(raw: &str) -> Self {
        Self::new(raw, false)
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn window(&self, top: usize, height: usize, left: usize, width: usize) -> Vec<Line<'static>> {
        (top..(top + height).min(self.lines.len())).map(|i| self.line(i, left, width)).collect()
    }

    fn line(&self, i: usize, left: usize, width: usize) -> Line<'static> {
        let text = &self.text[self.lines[i].clone()];
        let (mut spans, mut col, end) = (Vec::new(), 0, left + width);
        if !self.color {
            push_window(&mut spans, text, Style::default(), &mut col, left, end);
            return Line::from(spans);
        }
        let (mut style, mut rest) = (self.states[i], text);
        loop {
            let (plain, esc, tail) = split_sgr(rest);
            push_window(&mut spans, plain, style, &mut col, left, end);
            match esc {
                Some(p) if col < end => style = apply_sgr(style, p),
                _ => return Line::from(spans),
            }
            rest = tail;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_sgr_only() {
        assert_eq!(strip_ansi("\x1b[1;34m{\x1b[0m\x1b[m x \x1b[Kok"), "{ x \x1b[Kok");
    }

    #[test]
    fn sgr_colors_and_modifiers() {
        let s = apply_sgr(Style::default(), "1;32");
        assert_eq!(s.fg, Some(Color::Green));
        assert!(s.add_modifier.contains(Modifier::BOLD));
        assert_eq!(apply_sgr(s, "").fg, None);
        assert_eq!(apply_sgr(Style::default(), "38;5;208").fg, Some(Color::Indexed(208)));
        assert_eq!(apply_sgr(Style::default(), "48;2;1;2;3").bg, Some(Color::Rgb(1, 2, 3)));
        assert_eq!(apply_sgr(Style::default(), "94").fg, Some(Color::LightBlue));
        assert_eq!(apply_sgr(Style::default(), "90").fg, Some(Color::DarkGray));
    }

    #[test]
    fn state_carries_across_lines() {
        let d = Document::new("\x1b[31mred\nstill red\x1b[0m\nplain", true);
        let l = d.window(1, 1, 0, 80);
        assert_eq!(l[0].spans[0].style.fg, Some(Color::Red));
        assert_eq!(d.window(2, 1, 0, 80)[0].spans[0].style.fg, None);
        assert_eq!(d.width(), 9);
    }

    #[test]
    fn windowing_clips_columns() {
        let d = Document::new("\x1b[32mabcdef\x1b[0mghi", true);
        let l = d.window(0, 1, 2, 5);
        let text: String = l[0].spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(text, "cdefg");
    }

    #[test]
    fn mono_document_has_no_escapes() {
        let d = Document::new("\x1b[1mx\x1b[0m\n", false);
        assert_eq!(d.text(), "x");
        assert_eq!(d.len(), 1);
    }
}

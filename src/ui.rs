use crate::ansi::highlight_line;
use crate::app::{App, Focus, MAX_VISIBLE_ROWS, Pane, SearchMode};
use crate::jq::Options;
use crate::theme::Theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Clear, List, ListState, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};

const HINTS: &str = "^Space=complete Enter=accept ^J=newline ^S=save ^F=reload";

// One row per text line plus borders, between 3 rows and half the terminal.
pub fn filter_height(rows: u16, lines: usize) -> u16 {
    let wanted = u16::try_from(lines).unwrap_or(u16::MAX).saturating_add(2);
    wanted.clamp(3, (rows / 2).max(3))
}

fn block(title: &str, focused: bool, app: &App) -> Block<'static> {
    let style = if focused { app.theme.border_focus } else { app.theme.border };
    Block::bordered().border_type(BorderType::Rounded).border_style(style).title(format!(" {title} "))
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let [filter_area, lower, status_area] = Layout::vertical([
        Constraint::Length(filter_height(area.height, app.editor.line_count())),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);
    let [json_area, results_area] =
        Layout::horizontal([Constraint::Percentage(app.split_pct), Constraint::Min(0)]).areas(lower);
    app.rects.filter = filter_area;
    app.rects.json = json_area;
    app.rects.results = results_area;

    draw_filter(frame, app, filter_area);
    let (json_focus, results_focus) = (app.focus == Focus::Json, app.focus == Focus::Results);
    let (theme, json_block, results_block) = (
        app.theme,
        with_search_title(block("JSON Input", json_focus, app), &app.json_pane),
        with_search_title(block("Result", results_focus, app), &app.results_pane),
    );
    draw_pane(frame, json_area, json_block, &mut app.json_pane, &theme);
    draw_pane(frame, results_area, results_block, &mut app.results_pane, &theme);
    frame.render_widget(status_line(app, status_area.width), status_area);

    if app.completion.open {
        draw_completion(frame, app, filter_area);
    }
    if app.prompt.is_some() {
        draw_prompt(frame, app, area);
    }
}

fn draw_filter(frame: &mut Frame, app: &App, area: Rect) {
    let block = block("jq Filter", app.focus == Focus::Filter, app);
    let inner = block.inner(area);
    let (row, col) = app.editor.row_col();
    let (voff, hoff) = (
        (row + 1).saturating_sub(usize::from(inner.height)),
        (col + 3).saturating_sub(usize::from(inner.width)),
    );
    let lines: Vec<Line> = app
        .editor
        .text()
        .split('\n')
        .enumerate()
        .map(|(i, text)| {
            let prefix = if i == 0 { Span::styled("> ", app.theme.border_focus) } else { Span::raw("  ") };
            Line::from(vec![prefix, Span::raw(text.to_string())])
        })
        .collect();
    frame.render_widget(
        Paragraph::new(lines).style(app.theme.text).scroll((voff as u16, hoff as u16)).block(block),
        area,
    );
    if app.focus == Focus::Filter && app.prompt.is_none() {
        let x = inner.x + (col + 2 - hoff.min(col + 2)) as u16;
        let y = inner.y + (row - voff) as u16;
        frame.set_cursor_position(Position::new(
            x.min(inner.right().saturating_sub(1)),
            y.min(inner.bottom().saturating_sub(1)),
        ));
    }
}

// Shows the query and hit counter in the bottom border while a search is active.
fn with_search_title(block: Block<'static>, pane: &Pane) -> Block<'static> {
    let Some(search) = &pane.search else { return block };
    let cursor = if search.mode == SearchMode::Typing { "_" } else { "" };
    let hits = match search.matches.len() {
        0 => "no match".to_string(),
        n => format!("{}/{n}", search.current + 1),
    };
    block.title_bottom(format!(" /{}{cursor} ({hits}) ", search.query))
}

// Marks the hits on visible line `line`, relative to the first visible column.
fn marks_for(pane: &Pane, line: usize, theme: &Theme) -> Vec<(std::ops::Range<usize>, Style)> {
    let Some(search) = &pane.search else { return Vec::new() };
    let lo = search.matches.partition_point(|m| m.line < line);
    let hi = search.matches.partition_point(|m| m.line <= line);
    (lo..hi)
        .map(|i| {
            let m = search.matches[i];
            let style = if i == search.current {
                Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
            } else {
                theme.hit
            };
            (m.col.saturating_sub(pane.scroll_x)..(m.col + m.len).saturating_sub(pane.scroll_x), style)
        })
        .collect()
}

fn draw_pane(frame: &mut Frame, area: Rect, block: Block<'static>, pane: &mut Pane, theme: &Theme) {
    let (text, error) = (theme.text, theme.error);
    let inner = block.inner(area);
    let (h, w) = (usize::from(inner.height), usize::from(inner.width));
    pane.view_h = h;
    pane.view_w = w;
    pane.scroll_y = pane.scroll_y.min(pane.doc.len().saturating_sub(h));
    pane.scroll_x = pane.scroll_x.min(pane.doc.width().saturating_sub(w));
    let mut lines = pane.doc.window(pane.scroll_y, h, pane.scroll_x, w);
    if pane.search.is_some() {
        lines = lines
            .into_iter()
            .enumerate()
            .map(|(i, line)| highlight_line(line, &marks_for(pane, pane.scroll_y + i, theme)))
            .collect();
    }
    frame
        .render_widget(Paragraph::new(lines).style(if pane.error { error } else { text }).block(block), area);
    if pane.doc.len() > h {
        let mut state = ScrollbarState::new(pane.doc.len() - h).position(pane.scroll_y);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight).begin_symbol(None).end_symbol(None),
            area.inner(Margin { vertical: 1, horizontal: 0 }),
            &mut state,
        );
    }
}

// "[r]aw" style without the brackets: an option that is off gets its key letter (the first letter)
// in colour; one that is on reads as plain bold text.
fn option_spans(app: &App, label: &str, on: bool) -> Vec<Span<'static>> {
    if on {
        return vec![Span::styled(label.to_string(), app.theme.option_on)];
    }
    let (key, rest) = label.split_at(1);
    vec![
        Span::styled(key.to_string(), app.theme.toggle_off),
        Span::styled(rest.to_string(), app.theme.option_off),
    ]
}

// One line, capped at a third of the bar so the option labels and hints stay visible.
fn flatten_message(msg: &str, width: u16) -> String {
    let flat = msg.split_whitespace().collect::<Vec<_>>().join(" ");
    let max = usize::from(width / 3).max(10);
    if flat.chars().count() <= max {
        return flat;
    }
    flat.chars().take(max - 1).chain(std::iter::once('…')).collect()
}

fn status_line(app: &App, width: u16) -> Line<'static> {
    let Options { raw, compact, sort_keys, slurp, color } = app.options;
    let mut spans = vec![
        Span::raw(" "),
        Span::styled("● ", if app.working() { app.theme.working } else { app.theme.idle }),
    ];
    if !app.status_message.is_empty() {
        spans.push(Span::raw(format!("{} | ", flatten_message(&app.status_message, width))));
    }
    spans.push(Span::raw(format!("json:{}  ", if app.json_valid { "valid" } else { "INVALID" })));
    for (i, (label, on)) in
        [("raw", raw), ("compact", compact), ("Sort", sort_keys), ("slurp", slurp), ("Color", color)]
            .into_iter()
            .enumerate()
    {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        spans.extend(option_spans(app, label, on));
    }
    spans.push(Span::raw("  "));
    spans.push(Span::styled(HINTS, app.theme.option_off));
    Line::from(spans).style(app.theme.status)
}

fn draw_completion(frame: &mut Frame, app: &App, filter_area: Rect) {
    let screen = frame.area();
    let rows = app.completion.filtered.len().min(MAX_VISIBLE_ROWS) as u16;
    let width = (screen.width * 60 / 100).max(24).min(screen.width.saturating_sub(2));
    let area = Rect::new(filter_area.x + 2, filter_area.bottom(), width, rows + 2).intersection(screen);
    let list = List::new(app.completion.filtered.iter().map(String::as_str))
        .style(app.theme.text)
        .highlight_style(app.theme.selected)
        .block(block("jq paths", true, app));
    let mut state = ListState::default().with_selected(Some(app.completion.selected));
    frame.render_widget(Clear, area);
    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_prompt(frame: &mut Frame, app: &App, screen: Rect) {
    let Some(prompt) = &app.prompt else { return };
    let width = (screen.width * 70 / 100).max(20).min(screen.width);
    let area = Rect::new(
        screen.x + (screen.width - width) / 2,
        screen.y + screen.height.saturating_sub(3) / 2,
        width,
        3,
    )
    .intersection(screen);
    let block = block("save results to path", true, app);
    let inner = block.inner(area);
    let offset = (prompt.editor.cursor() + 1).saturating_sub(usize::from(inner.width));
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(prompt.editor.text().to_string())
            .style(app.theme.text)
            .scroll((0, offset as u16))
            .block(block),
        area,
    );
    frame.set_cursor_position(Position::new(inner.x + (prompt.editor.cursor() - offset) as u16, inner.y));
}

#[cfg(test)]
mod tests {
    use super::filter_height;

    #[test]
    fn filter_height_grows_with_lines_up_to_half_the_terminal() {
        assert_eq!(filter_height(24, 1), 3);
        assert_eq!(filter_height(24, 2), 4);
        assert_eq!(filter_height(24, 5), 7);
        assert_eq!(filter_height(24, 50), 12);
        assert_eq!(filter_height(48, 50), 24);
        assert_eq!(filter_height(4, 9), 3);
    }
}

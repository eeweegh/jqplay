use crate::app::{App, Focus, MAX_VISIBLE_ROWS, Pane};
use crate::jq::Options;
use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Position, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Clear, List, ListState, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState,
};

const HINTS: &str = "Tab=complete ^N/^B=focus ^Q=quit ^S=save ^F=reload Esc=dismiss";

pub fn filter_height(rows: u16) -> u16 {
    (rows / 4).min(12).checked_div(2).unwrap_or(0).max(3)
}

fn block(title: &str, focused: bool, app: &App) -> Block<'static> {
    let style = if focused { app.theme.border_focus } else { app.theme.border };
    Block::bordered().border_type(BorderType::Rounded).border_style(style).title(format!(" {title} "))
}

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let [filter_area, lower, status_area] = Layout::vertical([
        Constraint::Length(filter_height(area.height)),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);
    let [json_area, results_area] =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).areas(lower);
    app.rects.filter = filter_area;
    app.rects.json = json_area;
    app.rects.results = results_area;

    draw_filter(frame, app, filter_area);
    let (json_focus, results_focus) = (app.focus == Focus::Json, app.focus == Focus::Results);
    let (theme, json_block, results_block) =
        (app.theme, block("JSON Input", json_focus, app), block("Result", results_focus, app));
    draw_pane(frame, json_area, json_block, &mut app.json_pane, theme.text, theme.error);
    draw_pane(frame, results_area, results_block, &mut app.results_pane, theme.text, theme.error);
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
    let width = usize::from(inner.width);
    let offset = (app.editor.cursor() + 3).saturating_sub(width);
    let line = Line::from(vec![
        Span::styled("> ", app.theme.border_focus),
        Span::raw(app.editor.text().to_string()),
    ]);
    frame.render_widget(
        Paragraph::new(line).style(app.theme.text).scroll((0, offset as u16)).block(block),
        area,
    );
    if app.focus == Focus::Filter && app.prompt.is_none() {
        let x = inner.x + (app.editor.cursor() + 2 - offset.min(app.editor.cursor() + 2)) as u16;
        frame.set_cursor_position(Position::new(x.min(inner.right().saturating_sub(1)), inner.y));
    }
}

fn draw_pane(
    frame: &mut Frame,
    area: Rect,
    block: Block<'static>,
    pane: &mut Pane,
    text: Style,
    error: Style,
) {
    let inner = block.inner(area);
    let (h, w) = (usize::from(inner.height), usize::from(inner.width));
    pane.view_h = h;
    pane.scroll_y = pane.scroll_y.min(pane.doc.len().saturating_sub(h));
    pane.scroll_x = pane.scroll_x.min(pane.doc.width().saturating_sub(w));
    let lines = pane.doc.window(pane.scroll_y, h, pane.scroll_x, w);
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

fn option_span(app: &App, label: &str, key: char, on: bool) -> Span<'static> {
    Span::styled(format!("{label} [{key}]"), if on { app.theme.option_on } else { app.theme.option_off })
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
    for (i, (label, key, on)) in [
        ("raw", 'r', raw),
        ("compact", 'c', compact),
        ("sort", 'S', sort_keys),
        ("slurp", 's', slurp),
        ("color", 'C', color),
    ]
    .into_iter()
    .enumerate()
    {
        if i > 0 {
            spans.push(Span::raw(" "));
        }
        spans.push(option_span(app, label, key, on));
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
    fn filter_height_follows_terminal_rows() {
        assert_eq!(filter_height(24), 3);
        assert_eq!(filter_height(48), 6);
        assert_eq!(filter_height(200), 6);
        assert_eq!(filter_height(8), 3);
    }
}

use super::pick::{add_constraint, pick};
use super::{App, Focus};
use crate::editor::PROMPT_WIDTH;
use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};
use std::time::{Duration, Instant};

// Two left clicks on the same row of the input pane within this time are a double-click.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

// Percentage of `lower` left of `column`, keeping at least MIN_PANE_WIDTH columns on each side.
pub fn split_for_column(column: u16, lower: Rect) -> u16 {
    const MIN_PANE_WIDTH: u32 = 8;
    let width = u32::from(lower.width).max(1);
    let lo = (MIN_PANE_WIDTH * 100).div_ceil(width).clamp(10, 50);
    let pct = u32::from(column.saturating_sub(lower.x)) * 100 / width;
    u16::try_from(pct.clamp(lo, 100 - lo)).unwrap_or(50)
}

impl App {
    // The shared border of the two panes, with one column of slack on each side.
    pub(super) fn on_divider(&self, at: Position) -> bool {
        let (json, results) = (self.rects.json, self.rects.results);
        let rows = json.y..json.bottom();
        rows.contains(&at.y) && at.x + 2 >= results.x && at.x <= results.x + 1
    }

    // The pane whose scrollbar track is under `at`.
    fn bar_at(&self, at: Position) -> Option<Focus> {
        [(Focus::Json, &self.json_pane), (Focus::Results, &self.results_pane)]
            .into_iter()
            .find(|(_, p)| p.bar.is_some_and(|b| b.contains(at)))
            .map(|(f, _)| f)
    }

    fn drag_bar(&mut self, row: u16) {
        match self.dragging_bar {
            Some(Focus::Json) => self.json_pane.scroll_to_bar_row(row),
            Some(Focus::Results) => self.results_pane.scroll_to_bar_row(row),
            _ => {}
        }
    }

    // A click in the filter text puts the cursor there (the pane scrolls to keep the cursor in view, which
    // is undone here the same way the drawing does it).
    fn click_filter(&mut self, at: Position) {
        self.completion.close();
        let area = self.rects.filter;
        let inner =
            Rect::new(area.x + 1, area.y + 1, area.width.saturating_sub(2), area.height.saturating_sub(2));
        if !inner.contains(at) {
            return;
        }
        let (top, left) = self.editor.scroll(usize::from(inner.height), usize::from(inner.width));
        let row = top + usize::from(at.y - inner.y);
        let col = (left + usize::from(at.x - inner.x)).saturating_sub(PROMPT_WIDTH);
        self.editor.set_row_col(row, col);
    }

    // A double-click on a scalar of the input pane adds "that key has that value" to the query.
    fn click_input_pane(&mut self, at: Position) {
        let now = Instant::now();
        let previous = self.last_click.take();
        if !previous.is_some_and(|(t, p)| p.y == at.y && now.duration_since(t) <= DOUBLE_CLICK) {
            self.last_click = Some((now, at));
            return;
        }
        let area = self.rects.json;
        let row = usize::from(at.y.saturating_sub(area.y + 1));
        let inside = at.y > area.y && at.y + 1 < area.bottom();
        if !inside || self.json_pane.error {
            return;
        }
        let line = self.json_pane.scroll_y + row;
        match pick(&self.json_pane.doc, line) {
            Some(picked) => {
                let query = add_constraint(self.editor.text(), &picked);
                self.editor.set_text(&query);
                self.run_now();
            }
            None => self.status_message = "double-click a value (string, number, true, false or null)".into(),
        }
    }

    pub(super) fn drag_divider(&mut self, column: u16) {
        let lower = self.rects.json.union(self.rects.results);
        self.split_pct = split_for_column(column, lower);
    }

    pub(super) fn on_mouse(&mut self, mouse: MouseEvent) {
        if self.prompt.is_some() {
            return;
        }
        let at = Position::new(mouse.column, mouse.row);
        let target = [
            (Focus::Filter, self.rects.filter),
            (Focus::Json, self.rects.json),
            (Focus::Results, self.rects.results),
        ]
        .into_iter()
        .find(|(_, r)| r.contains(at))
        .map(|(f, _)| f);
        let bar = self.bar_at(at);
        match (mouse.kind, target) {
            (MouseEventKind::Down(MouseButton::Left), _) if bar.is_some() => {
                self.dragging_bar = bar;
                if let Some(f) = bar.filter(|&f| f != self.focus) {
                    self.set_focus(f);
                }
                self.drag_bar(at.y);
            }
            (MouseEventKind::Drag(MouseButton::Left), _) if self.dragging_bar.is_some() => {
                self.drag_bar(at.y);
            }
            (MouseEventKind::Down(MouseButton::Left), _) if self.on_divider(at) => self.dragging = true,
            (MouseEventKind::Drag(MouseButton::Left), _) if self.dragging => self.drag_divider(at.x),
            (MouseEventKind::Up(MouseButton::Left), _) => {
                self.dragging = false;
                self.dragging_bar = None;
            }
            (MouseEventKind::Down(MouseButton::Left), Some(f)) => {
                if f != self.focus {
                    self.set_focus(f);
                }
                match f {
                    Focus::Filter => self.click_filter(at),
                    Focus::Json => self.click_input_pane(at),
                    Focus::Results => {}
                }
            }
            (MouseEventKind::ScrollUp | MouseEventKind::ScrollDown, Some(f)) => {
                let down = mouse.kind == MouseEventKind::ScrollDown;
                match f {
                    Focus::Json => self.json_pane.wheel(down),
                    Focus::Results => self.results_pane.wheel(down),
                    Focus::Filter => {}
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod split_tests {
    use super::split_for_column;
    use ratatui::layout::Rect;

    #[test]
    fn split_follows_column_and_keeps_both_panes_usable() {
        let lower = Rect::new(0, 1, 100, 20);
        assert_eq!(split_for_column(30, lower), 30);
        assert_eq!(split_for_column(0, lower), 10);
        assert_eq!(split_for_column(99, lower), 90);
        let narrow = Rect::new(0, 1, 40, 20);
        assert_eq!(split_for_column(1, narrow), 20);
    }
}

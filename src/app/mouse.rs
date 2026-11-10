use super::{App, Focus};
use ratatui::crossterm::event::{MouseButton, MouseEvent, MouseEventKind};
use ratatui::layout::{Position, Rect};

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
            (MouseEventKind::Down(MouseButton::Left), Some(f)) if f != self.focus => self.set_focus(f),
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

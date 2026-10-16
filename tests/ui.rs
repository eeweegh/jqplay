mod support;

use jqplay::app::{Event, Focus, Outcome};
use ratatui::crossterm::event::{
    Event as TermEvent, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use support::{SAMPLE, app_with, ctrl, flush_debounce, focus_next, focus_prev, press, render, type_text};

#[test]
fn startup_with_filter_shows_input_and_result() {
    let mut app = app_with(Some(SAMPLE), &["--filter", ".users[].name"]);
    insta::assert_snapshot!(render(&mut app, 90, 16));
}

#[test]
fn no_input_shows_empty_panes_focused_on_json() {
    let mut app = app_with(None, &[]);
    insta::assert_snapshot!(render(&mut app, 90, 12));
}

#[test]
fn invalid_json_reports_error_and_marks_status() {
    let mut app = app_with(Some("{oops"), &["--filter", "."]);
    let screen = render(&mut app, 100, 12);
    assert!(screen.contains("json:INVALID"));
    assert!(screen.contains("parse error"));
}

#[test]
fn typing_is_debounced_then_runs() {
    let mut app = app_with(Some(SAMPLE), &[]);
    type_text(&mut app, ".count");
    assert!(!render(&mut app, 90, 12).contains(" 2\n"));
    flush_debounce(&mut app);
    assert!(render(&mut app, 90, 12).lines().any(|l| l.contains("│2")));
}

#[test]
fn jq_error_is_shown_in_result_pane() {
    let mut app = app_with(Some(SAMPLE), &["--filter", ".users | nope"]);
    assert!(render(&mut app, 100, 12).contains("nope"));
}

#[test]
fn popup_opens_while_typing_and_enter_accepts() {
    let mut app = app_with(Some(SAMPLE), &[]);
    type_text(&mut app, ".us");
    assert!(app.completion.open);
    insta::assert_snapshot!(render(&mut app, 70, 14));
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.editor.text(), ".users");
    assert!(!app.completion.open);
}

#[test]
fn boundary_char_closes_popup_and_is_inserted() {
    let mut app = app_with(Some(SAMPLE), &[]);
    type_text(&mut app, ".us");
    assert!(app.completion.open);
    type_text(&mut app, " ");
    assert!(!app.completion.open);
    assert_eq!(app.editor.text(), ".us ");
}

#[test]
fn toggles_flip_status_and_output() {
    let mut app = app_with(Some(SAMPLE), &["--filter", ".users[0].name"]);
    assert!(render(&mut app, 100, 12).lines().any(|l| l.contains("\"ann\"")));
    focus_next(&mut app);
    type_text(&mut app, "r");
    let screen = render(&mut app, 100, 12);
    assert!(screen.lines().any(|l| l.contains("│ann")));
    assert!(app.options.raw);
}

#[test]
fn readline_keys_win_over_toggles_in_filter_pane() {
    let mut app = app_with(Some(SAMPLE), &["--filter", ".count"]);
    ctrl(&mut app, 'k');
    assert!(!app.options.sort_keys);
    assert_eq!(app.editor.text(), ".count");
    ctrl(&mut app, 'a');
    ctrl(&mut app, 'k');
    assert_eq!(app.editor.text(), "");
    ctrl(&mut app, 'y');
    assert_eq!(app.editor.text(), ".count");
}

#[test]
fn single_key_toggles_only_outside_filter_pane() {
    let mut app = app_with(Some(SAMPLE), &[]);
    type_text(&mut app, "rcsSC");
    assert_eq!(app.options, jqplay::jq::Options::default());
    assert_eq!(app.editor.text(), "rcsSC");
    focus_next(&mut app);
    type_text(&mut app, "rcsSC");
    assert_eq!(
        app.options,
        jqplay::jq::Options { raw: true, compact: true, sort_keys: true, slurp: true, color: true }
    );
    focus_next(&mut app);
    type_text(&mut app, "r");
    assert!(!app.options.raw);
}

#[test]
fn old_ctrl_toggles_are_gone() {
    let mut app = app_with(Some(SAMPLE), &[]);
    focus_next(&mut app);
    for c in ['x', 'p', 'k', 'u', 'g'] {
        ctrl(&mut app, c);
    }
    assert_eq!(app.options, jqplay::jq::Options::default());
}

#[test]
fn ctrl_q_returns_filter_and_ctrl_c_aborts() {
    let mut app = app_with(Some(SAMPLE), &["--filter", ".count"]);
    ctrl(&mut app, 'q');
    assert_eq!(app.outcome(), Some(&Outcome::Clean(".count".into())));
    let mut app = app_with(Some(SAMPLE), &[]);
    ctrl(&mut app, 'c');
    assert_eq!(app.outcome(), Some(&Outcome::Abort));
}

#[test]
fn save_writes_plain_result_text() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("out/result.txt");
    let mut app = app_with(Some(SAMPLE), &["--filter", ".count"]);
    focus_next(&mut app);
    ctrl(&mut app, 'y');
    assert!(app.prompt.is_some());
    for _ in 0..20 {
        press(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
    }
    type_text(&mut app, target.to_str().unwrap());
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "2\n");
}

#[test]
fn nothing_to_save_without_results() {
    let mut app = app_with(Some(SAMPLE), &[]);
    focus_next(&mut app);
    ctrl(&mut app, 'y');
    assert!(app.prompt.is_none());
    assert_eq!(app.status_message, "nothing to save (no valid results)");
}

#[test]
fn reload_from_stdin_is_refused() {
    let mut app = app_with(Some(SAMPLE), &[]);
    ctrl(&mut app, 'f');
    assert!(app.status_message.contains("no source file"));
}

#[test]
fn reload_rereads_the_source_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("in.json");
    std::fs::write(&file, "{\"v\":1}").unwrap();
    let mut app = app_with(Some("{\"v\":1}"), &["--filter", ".v", file.to_str().unwrap()]);
    std::fs::write(&file, "{\"v\":42}").unwrap();
    ctrl(&mut app, 'f');
    assert!(render(&mut app, 100, 12).lines().any(|l| l.contains("42")));
    assert!(app.status_message.starts_with("reloaded"));
}

#[test]
fn end_key_scrolls_to_the_true_bottom() {
    let json = format!("[{}]", (0..200).map(|i| i.to_string()).collect::<Vec<_>>().join(","));
    let mut app = app_with(Some(&json), &["--filter", ".[]"]);
    focus_prev(&mut app);
    focus_prev(&mut app);
    press(&mut app, KeyCode::End, KeyModifiers::NONE);
    assert!(render(&mut app, 100, 12).contains("199"));
}

#[test]
fn esc_dismisses_status_message_and_popup() {
    let mut app = app_with(Some(SAMPLE), &[]);
    focus_next(&mut app);
    ctrl(&mut app, 'y');
    assert!(!app.status_message.is_empty());
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.status_message.is_empty());
    focus_prev(&mut app);
    type_text(&mut app, ".us");
    assert!(app.completion.open);
    app.status_message = "stale".into();
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!app.completion.open && app.status_message.is_empty());
}

#[test]
fn long_multiline_message_keeps_status_bar_intact() {
    let mut app = app_with(Some(SAMPLE), &[]);
    app.status_message = format!("jq: error: {}\nsecond line", "boom ".repeat(60));
    let screen = render(&mut app, 140, 12);
    let last = screen.lines().last().unwrap();
    assert!(last.contains("json:valid") && last.contains("Color") && last.contains('…'), "{last}");
    assert_eq!(screen.lines().count(), 12);
}

#[test]
fn ctrl_s_saves_from_the_filter_pane() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("saved.txt");
    let mut app = app_with(Some(SAMPLE), &["--filter", ".count"]);
    ctrl(&mut app, 's');
    assert!(app.prompt.is_some());
    for _ in 0..20 {
        press(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
    }
    type_text(&mut app, target.to_str().unwrap());
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "2\n");
    assert!(app.status_message.starts_with("saved to"));
}

#[test]
fn filter_line_autocloses_and_steps_over() {
    let mut app = app_with(Some(SAMPLE), &[]);
    type_text(&mut app, ".users[");
    assert_eq!((app.editor.text(), app.editor.cursor()), (".users[]", 7));
    type_text(&mut app, "]");
    assert_eq!((app.editor.text(), app.editor.cursor()), (".users[]", 8));
    type_text(&mut app, " | select(.name == \"ann\")");
    assert_eq!(app.editor.text(), ".users[] | select(.name == \"ann\")");
}

#[test]
fn completion_still_opens_next_to_paired_brackets() {
    let mut app = app_with(Some(SAMPLE), &[]);
    type_text(&mut app, "(.us");
    assert_eq!(app.editor.text(), "(.us)");
    assert!(app.completion.open);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.editor.text(), "(.users)");
}

#[test]
fn enter_in_filter_and_q_in_view_panes_accept() {
    let mut app = app_with(Some(SAMPLE), &["--filter", ".count"]);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.outcome(), Some(&Outcome::Clean(".count".into())));
    let mut app = app_with(Some(SAMPLE), &["--filter", ".count"]);
    type_text(&mut app, "q");
    assert_eq!(app.outcome(), None);
    assert_eq!(app.editor.text(), ".countq");
    focus_next(&mut app);
    type_text(&mut app, "q");
    assert_eq!(app.outcome(), Some(&Outcome::Clean(".countq".into())));
}

#[test]
fn enter_with_popup_open_accepts_the_suggestion_not_the_filter() {
    let mut app = app_with(Some(SAMPLE), &[]);
    type_text(&mut app, ".us");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.outcome(), None);
    assert_eq!(app.editor.text(), ".users");
}

#[test]
fn tab_cycles_focus_and_shift_tab_goes_back() {
    let mut app = app_with(Some(SAMPLE), &[]);
    assert_eq!(app.focus, Focus::Filter);
    focus_next(&mut app);
    assert_eq!(app.focus, Focus::Json);
    focus_next(&mut app);
    assert_eq!(app.focus, Focus::Results);
    focus_next(&mut app);
    assert_eq!(app.focus, Focus::Filter);
    focus_prev(&mut app);
    assert_eq!(app.focus, Focus::Results);
}

#[test]
fn tab_steps_through_matches_while_popup_is_open() {
    let mut app = app_with(Some(SAMPLE), &[]);
    type_text(&mut app, ".users");
    assert!(app.completion.open);
    let first = app.completion.selected;
    focus_next(&mut app);
    assert_eq!(app.focus, Focus::Filter);
    assert_eq!(app.completion.selected, first + 1);
    focus_prev(&mut app);
    assert_eq!(app.completion.selected, first);
}

#[test]
fn ctrl_space_opens_popup_when_autocomplete_is_off() {
    let mut app = app_with(Some(SAMPLE), &[]);
    app.autocomplete = false;
    type_text(&mut app, ".us");
    assert!(!app.completion.open);
    ctrl(&mut app, ' ');
    assert!(app.completion.open);
}

#[test]
fn autopopup_skips_strings_and_exact_matches() {
    let mut app = app_with(Some(SAMPLE), &[]);
    type_text(&mut app, "select(.name == \"a");
    assert!(!app.completion.open);
    let mut app = app_with(Some(SAMPLE), &[]);
    type_text(&mut app, ".count");
    assert!(!app.completion.open);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.outcome(), Some(&Outcome::Clean(".count".into())));
}

#[test]
fn enter_with_popup_open_never_exits_even_if_selection_equals_token() {
    let mut app = app_with(Some(SAMPLE), &[]);
    type_text(&mut app, ".users");
    assert!(app.completion.open);
    assert_eq!(app.completion.choice(), Some(".users"));
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.outcome(), None);
    assert!(!app.completion.open);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.outcome(), Some(&Outcome::Clean(".users".into())));
}

fn filter_rows(screen: &str) -> usize {
    screen.lines().position(|l| l.trim_start_matches('"').starts_with('╰')).map_or(0, |i| i + 1)
}

fn newline(app: &mut jqplay::app::App, how: usize) {
    match how {
        0 => ctrl(app, 'j'),
        1 => press(app, KeyCode::Enter, KeyModifiers::ALT),
        _ => press(app, KeyCode::Enter, KeyModifiers::SHIFT),
    }
}

#[test]
fn every_newline_key_adds_a_line_and_the_pane_grows_then_shrinks() {
    for how in 0..3 {
        let mut app = app_with(Some(SAMPLE), &[]);
        assert_eq!(filter_rows(&render(&mut app, 80, 24)), 3);
        type_text(&mut app, ".users[]");
        newline(&mut app, how);
        type_text(&mut app, "| .name");
        assert_eq!(app.outcome(), None);
        assert_eq!(app.editor.text(), ".users[]\n| .name");
        assert_eq!(filter_rows(&render(&mut app, 80, 24)), 4);
        press(&mut app, KeyCode::Home, KeyModifiers::NONE);
        press(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(app.editor.text(), ".users[]| .name");
        assert_eq!(filter_rows(&render(&mut app, 80, 24)), 3);
    }
}

#[test]
fn three_line_filter_renders_with_continuation_indent() {
    let mut app = app_with(Some(SAMPLE), &[]);
    type_text(&mut app, ".users[]");
    ctrl(&mut app, 'j');
    type_text(&mut app, "| select(.age > 30)");
    ctrl(&mut app, 'j');
    type_text(&mut app, "| .name");
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    flush_debounce(&mut app);
    insta::assert_snapshot!(render(&mut app, 70, 16));
}

#[test]
fn multi_line_filter_runs_in_jq() {
    let mut app = app_with(Some(SAMPLE), &[]);
    type_text(&mut app, ".users[]");
    ctrl(&mut app, 'j');
    type_text(&mut app, "| .name");
    flush_debounce(&mut app);
    let screen = render(&mut app, 100, 16);
    assert!(screen.lines().any(|l| l.contains("\"ann\"")) && screen.contains("\"bob\""));
}

#[test]
fn up_and_down_move_between_lines_before_touching_history() {
    let mut app = app_with(Some(SAMPLE), &[]);
    type_text(&mut app, "ab");
    ctrl(&mut app, 'j');
    type_text(&mut app, "cd");
    press(&mut app, KeyCode::Up, KeyModifiers::NONE);
    assert_eq!(app.editor.row_col(), (0, 2));
    press(&mut app, KeyCode::Down, KeyModifiers::NONE);
    assert_eq!(app.editor.row_col(), (1, 2));
    assert_eq!(app.editor.text(), "ab\ncd");
}

#[test]
fn paste_keeps_newlines_in_the_filter() {
    use jqplay::app::Event;
    use ratatui::crossterm::event::Event as TermEvent;
    let mut app = app_with(Some(SAMPLE), &[]);
    app.handle(Event::Term(TermEvent::Paste(".users[]\r\n| .name\n".into())));
    assert_eq!(app.editor.text(), ".users[]\n| .name\n");
}

#[test]
fn tall_filter_is_capped_at_half_the_screen_and_keeps_the_cursor_visible() {
    let mut app = app_with(Some(SAMPLE), &[]);
    for i in 1..=15 {
        type_text(&mut app, &format!("l{i}"));
        if i < 15 {
            ctrl(&mut app, 'j');
        }
    }
    let screen = render(&mut app, 80, 20);
    assert_eq!(filter_rows(&screen), 10);
    assert!(screen.contains("l15") && !screen.contains("> l1 "));
}

fn mouse(app: &mut jqplay::app::App, kind: MouseEventKind, column: u16, row: u16) {
    app.handle(Event::Term(TermEvent::Mouse(MouseEvent {
        kind,
        column,
        row,
        modifiers: KeyModifiers::NONE,
    })));
}

#[test]
fn dragging_the_divider_resizes_the_panes() {
    let mut app = app_with(Some(r#"{"a":1}"#), &[".a"]);
    render(&mut app, 100, 24);
    assert_eq!(app.rects.json.width, 50);
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), 50, 10);
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), 30, 10);
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), 30, 10);
    render(&mut app, 100, 24);
    assert_eq!(app.rects.json.width, 30);
    // Not dragging any more: further movement is ignored.
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), 70, 10);
    render(&mut app, 100, 24);
    assert_eq!(app.rects.json.width, 30);
}

fn long_doc() -> String {
    let items: Vec<String> = (0..60).map(|i| format!("\"item{i}\"")).collect();
    format!("[{}]", items.join(","))
}

#[test]
fn search_jumps_steps_wraps_and_ends_with_esc() {
    let mut app = app_with(Some(&long_doc()), &["--filter", ".[]", "--raw"]);
    render(&mut app, 80, 24);
    // Results pane, `r` while typing is literal and must not toggle raw.
    focus_next(&mut app);
    focus_next(&mut app);
    assert_eq!(app.focus, Focus::Results);
    type_text(&mut app, "/item5");
    let r = render(&mut app, 80, 24);
    eprintln!("{r}");
    assert!(r.contains("/item5_ (1/11)"));
    type_text(&mut app, "r");
    assert!(render(&mut app, 80, 24).contains("(no match)"));
    press(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    let committed = render(&mut app, 80, 24);
    assert!(committed.contains("/item5 (1/11)"));
    type_text(&mut app, "n");
    assert!(render(&mut app, 80, 24).contains("(2/11)"));
    type_text(&mut app, "N");
    type_text(&mut app, "N");
    assert!(render(&mut app, 80, 24).contains("(11/11)"));
    // The other pane has its own, untouched search state.
    focus_prev(&mut app);
    assert!(app.json_pane.search.is_none() && app.results_pane.search.is_some());
    focus_next(&mut app);
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(!render(&mut app, 80, 24).contains("/item5"));
}

#[test]
fn n_toggles_nothing_and_slash_is_per_pane() {
    let mut app = app_with(Some(&long_doc()), &["--filter", ".[]"]);
    focus_next(&mut app);
    type_text(&mut app, "/item1");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    type_text(&mut app, "c");
    // Navigating mode still lets option keys through.
    assert!(render(&mut app, 100, 24).contains("compact"));
    assert!(render(&mut app, 100, 24).contains("/item1"));
}

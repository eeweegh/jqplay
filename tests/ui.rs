mod support;

use jqplay::app::{Event, Focus, Outcome};
use jqplay::history::FilterHistory;
use ratatui::crossterm::event::{
    Event as TermEvent, KeyCode, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use support::{
    SAMPLE, app_with, ctrl, flush_debounce, focus_next, focus_prev, press, render, status_style, type_text,
};

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
fn saving_over_an_existing_file_needs_a_second_enter() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("result.json");
    std::fs::write(&target, "old").unwrap();
    let mut app = app_with(Some(SAMPLE), &["--filter", ".count"]);
    ctrl(&mut app, 's');
    for _ in 0..20 {
        press(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
    }
    type_text(&mut app, target.to_str().unwrap());
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.prompt.as_ref().is_some_and(|p| p.overwrite));
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "old");
    // Editing the path disarms the confirmation; the next Enter asks again.
    press(&mut app, KeyCode::Backspace, KeyModifiers::NONE);
    assert!(app.prompt.as_ref().is_some_and(|p| !p.overwrite));
    type_text(&mut app, "n");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.prompt.as_ref().is_some_and(|p| p.overwrite));
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "old");
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert!(app.prompt.is_none());
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "2\n");
}

#[test]
fn nothing_to_save_without_results() {
    let mut app = app_with(Some(SAMPLE), &[]);
    focus_next(&mut app);
    focus_next(&mut app);
    assert_eq!(app.focus, Focus::Results);
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
    focus_next(&mut app);
    ctrl(&mut app, 'y');
    assert!(!app.status_message.is_empty());
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.status_message.is_empty());
    focus_prev(&mut app);
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
fn ctrl_q_anywhere_and_q_in_view_panes_accept_but_enter_does_not() {
    let mut app = app_with(Some(SAMPLE), &["--filter", ".count"]);
    press(&mut app, KeyCode::Enter, KeyModifiers::NONE);
    assert_eq!(app.outcome(), None);
    assert_eq!(app.editor.text(), ".count\n");
    ctrl(&mut app, 'q');
    assert_eq!(app.outcome(), Some(&Outcome::Clean(".count\n".into())));
    let mut app = app_with(Some(SAMPLE), &["--filter", ".count"]);
    ctrl(&mut app, 'q');
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
    ctrl(&mut app, 'q');
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
    ctrl(&mut app, 'q');
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
fn space_and_b_page_the_focused_pane() {
    let mut app = app_with(Some(&long_doc()), &["--filter", ".[]"]);
    render(&mut app, 80, 24);
    focus_next(&mut app);
    assert_eq!(app.focus, Focus::Json);
    let page = app.json_pane.view_h - 1;
    type_text(&mut app, " ");
    assert_eq!(app.json_pane.scroll_y, page);
    type_text(&mut app, " ");
    assert_eq!(app.json_pane.scroll_y, 2 * page);
    type_text(&mut app, "b");
    assert_eq!(app.json_pane.scroll_y, page);
    assert_eq!(app.results_pane.scroll_y, 0);
}

#[test]
fn dragging_the_scrollbar_thumb_scrolls_the_pane() {
    let mut app = app_with(Some(&long_doc()), &["--filter", ".[]"]);
    render(&mut app, 100, 24);
    let bar = app.results_pane.bar.expect("results overflow, so the scrollbar is drawn");
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), bar.x, bar.y);
    assert_eq!(app.focus, Focus::Results);
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), bar.x, bar.y + bar.height - 1);
    render(&mut app, 100, 24);
    let max = app.results_pane.doc.len() - app.results_pane.view_h;
    assert_eq!(app.results_pane.scroll_y, max);
    // Dragging past the end of the track stays clamped; the json pane has its own bar.
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), bar.x, bar.y + bar.height + 5);
    assert_eq!(app.results_pane.scroll_y, max);
    mouse(&mut app, MouseEventKind::Up(MouseButton::Left), bar.x, bar.y);
    mouse(&mut app, MouseEventKind::Drag(MouseButton::Left), bar.x, bar.y);
    assert_eq!(app.results_pane.scroll_y, max);
    let json_bar = app.json_pane.bar.expect("json overflows too");
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), json_bar.x, json_bar.y + json_bar.height - 1);
    assert!(app.json_pane.scroll_y > 0);
    assert_eq!(app.rects.json.width, 50, "grabbing the json bar must not start a divider drag");
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

#[test]
fn history_is_an_array_edited_in_place_and_saved_on_accept_only() {
    use clap::Parser;
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("history");
    std::fs::write(&file, r#"[".a", ".b"]"#).unwrap();
    let start = |file: &std::path::Path| {
        let cli = jqplay::cli::Cli::parse_from(["jqplay", "--history-file", file.to_str().unwrap()]);
        let settings = jqplay::config::Settings::resolve(&cli, &jqplay::config::Config::default());
        let mut app = jqplay::app::App::new(&settings, Some(SAMPLE.to_string()));
        support::settle(&mut app);
        app
    };
    let mut app = start(&file);
    assert_eq!(app.focus, Focus::Filter);
    press(&mut app, KeyCode::Up, KeyModifiers::NONE);
    assert!(render(&mut app, 80, 24).contains("> .b"));
    type_text(&mut app, "x");
    press(&mut app, KeyCode::Up, KeyModifiers::NONE);
    press(&mut app, KeyCode::Down, KeyModifiers::NONE);
    assert!(render(&mut app, 80, 24).contains("> .bx"));
    // Nothing is written until a normal quit.
    assert_eq!(std::fs::read_to_string(&file).unwrap(), r#"[".a", ".b"]"#);
    ctrl(&mut app, 'q');
    assert!(matches!(app.outcome(), Some(Outcome::Clean(f)) if f == ".bx"));
    app.save_history();
    assert_eq!(FilterHistory::load(&file).entries(), [".a", ".b", ".bx", ""]);
}

#[test]
fn status_bar_shows_item_count_only_for_valid_json() {
    let mut app = app_with(Some(SAMPLE), &[]);
    assert!(render(&mut app, 100, 24).contains("json:valid (1)"));
    let mut app = app_with(Some("[1,2,3]\n"), &[]);
    assert!(render(&mut app, 100, 24).contains("json:valid (3)"));
    let mut app = app_with(Some("{oops"), &[]);
    let screen = render(&mut app, 100, 24);
    assert!(screen.contains("json:INVALID") && !screen.contains("INVALID ("));
}

#[test]
fn ctrl_z_requests_suspend_in_every_pane_and_mode() {
    let mut app = app_with(Some(SAMPLE), &["--filter", ".count"]);
    assert!(!app.take_suspend());
    for _ in 0..3 {
        ctrl(&mut app, 'z');
        assert!(app.take_suspend());
        assert!(!app.take_suspend());
        focus_next(&mut app);
    }
    assert_eq!(app.editor.text(), ".count");
    assert_eq!(app.outcome(), None);
    ctrl(&mut app, 's');
    ctrl(&mut app, 'z');
    assert!(app.take_suspend());
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    type_text(&mut app, "/ab");
    ctrl(&mut app, 'z');
    assert!(app.take_suspend());
    assert_eq!(app.outcome(), None);
}

#[test]
fn a_superseded_never_ending_filter_is_cancelled_and_replaced() {
    use jqplay::app::App;
    use jqplay::cli::Cli;
    use jqplay::config::{Config, Settings};
    use jqplay::job::execute;
    use ratatui::crossterm::event::KeyEvent;
    use std::sync::mpsc;
    use std::time::Duration;

    let cli = <Cli as clap::Parser>::parse_from([
        "jqplay",
        "--history-file",
        "",
        "--no-color",
        "--filter",
        "until(false; .)",
    ]);
    let mut app = App::new(&Settings::resolve(&cli, &Config::default()), Some(SAMPLE.into()));
    let (tx, rx) = mpsc::channel();
    let dispatch = |app: &mut App| {
        for job in app.take_jobs() {
            let tx = tx.clone();
            std::thread::spawn(move || tx.send(execute(job)).unwrap());
        }
    };
    dispatch(&mut app);
    // Replace the filter while its jq is still spinning.
    for _ in 0..20 {
        app.handle(Event::Term(TermEvent::Key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE))));
    }
    for c in ".count".chars() {
        app.handle(Event::Term(TermEvent::Key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))));
    }
    app.tick(app.deadline().expect("typing sets the debounce"));
    for _ in 0..20 {
        dispatch(&mut app);
        if render(&mut app, 90, 12).lines().any(|l| l.contains("│2")) {
            return;
        }
        let reply = rx.recv_timeout(Duration::from_secs(10)).expect("the old jq was not stopped");
        app.handle(Event::Reply(reply));
    }
    panic!("the new filter never ran");
}

#[test]
fn dropping_the_app_stops_running_jq_processes() {
    use jqplay::app::App;
    use jqplay::cli::Cli;
    use jqplay::config::{Config, Settings};
    use jqplay::job::execute;
    use std::sync::mpsc;
    use std::time::Duration;

    let cli =
        <Cli as clap::Parser>::parse_from(["jqplay", "--history-file", "", "--filter", "until(false; .)"]);
    let mut app = App::new(&Settings::resolve(&cli, &Config::default()), Some(SAMPLE.into()));
    let (tx, rx) = mpsc::channel();
    let jobs = app.take_jobs();
    let count = jobs.len();
    for job in jobs {
        let tx = tx.clone();
        std::thread::spawn(move || tx.send(execute(job)).unwrap());
    }
    std::thread::sleep(Duration::from_millis(200));
    drop(app);
    for _ in 0..count {
        rx.recv_timeout(Duration::from_secs(10)).expect("a jq outlived the app");
    }
}

fn save_as(app: &mut jqplay::app::App, path: &std::path::Path) {
    for _ in 0..30 {
        press(app, KeyCode::Backspace, KeyModifiers::NONE);
    }
    type_text(app, path.to_str().unwrap());
    press(app, KeyCode::Enter, KeyModifiers::NONE);
}

#[test]
fn ctrl_s_saves_what_the_focused_pane_shows() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with(Some(SAMPLE), &["--filter", ".count"]);
    focus_next(&mut app);
    assert_eq!(app.focus, Focus::Json);
    ctrl(&mut app, 's');
    let screen = render(&mut app, 90, 16);
    assert!(screen.contains("save input to path") && screen.contains("input.json"), "{screen}");
    let input = dir.path().join("in.json");
    save_as(&mut app, &input);
    let saved = std::fs::read_to_string(&input).unwrap();
    assert!(saved.starts_with("{\n  \"users\": [\n") && !saved.contains('\x1b'), "{saved}");
    assert_eq!(serde_json::from_str::<serde_json::Value>(&saved).unwrap()["count"], 2);

    focus_next(&mut app);
    assert_eq!(app.focus, Focus::Results);
    ctrl(&mut app, 's');
    let screen = render(&mut app, 90, 16);
    assert!(screen.contains("save results to path") && screen.contains("output.json"), "{screen}");
    let output = dir.path().join("out.json");
    save_as(&mut app, &output);
    assert_eq!(std::fs::read_to_string(&output).unwrap(), "2\n");
}

#[test]
fn the_remembered_save_path_is_per_pane() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with(Some(SAMPLE), &["--filter", ".count"]);
    focus_next(&mut app);
    ctrl(&mut app, 's');
    save_as(&mut app, &dir.path().join("in.json"));
    let proposed = |app: &jqplay::app::App| app.prompt.as_ref().unwrap().editor.text().to_string();
    ctrl(&mut app, 's');
    assert!(proposed(&app).ends_with("in.json"));
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    focus_next(&mut app);
    ctrl(&mut app, 's');
    assert_eq!(proposed(&app), "output.json");
}

#[test]
fn nothing_to_save_from_the_input_pane_when_the_json_is_invalid() {
    let mut app = app_with(Some("{oops"), &["--filter", "."]);
    focus_next(&mut app);
    ctrl(&mut app, 's');
    assert!(app.prompt.is_none());
    assert_eq!(app.status_message, "nothing to save (no valid input)");
}

#[test]
fn the_hotkey_letter_keeps_its_style_whether_or_not_the_option_is_on() {
    let mut app = app_with(Some(SAMPLE), &["--filter", "."]);
    let off = status_style(&mut app, 100, 12, "raw");
    let rest_off = status_style(&mut app, 100, 12, "aw");
    focus_next(&mut app);
    type_text(&mut app, "r");
    assert!(app.options.raw);
    let on = status_style(&mut app, 100, 12, "raw");
    let rest_on = status_style(&mut app, 100, 12, "aw");
    assert_eq!(off, on);
    assert_ne!(rest_off, rest_on);
    assert_ne!(on, rest_on, "the hotkey letter stands out from the label");
}

// Left-clicks on the input pane's content line `line`, as the terminal reports them.
fn click_json_line(app: &mut jqplay::app::App, line: u16) {
    let (x, y) = (app.rects.json.x + 3, app.rects.json.y + 1 + line);
    mouse(app, MouseEventKind::Down(MouseButton::Left), x, y);
    mouse(app, MouseEventKind::Up(MouseButton::Left), x, y);
}

fn double_click_json_line(app: &mut jqplay::app::App, line: u16) {
    click_json_line(app, line);
    click_json_line(app, line);
    support::settle(app);
}

// Input pane lines of SAMPLE pretty-printed: 3 = users[0].name, 4 = age, 7 = users[1].name, 8 = age, 11 = count.
#[test]
fn double_clicking_a_value_in_an_array_filters_on_it() {
    let mut app = app_with(Some(SAMPLE), &[]);
    render(&mut app, 100, 24);
    double_click_json_line(&mut app, 7);
    assert_eq!(app.editor.text(), "select(any(.users[]; .name == \"bob\"))");
    assert_eq!(app.focus, Focus::Json);
    assert!(app.results_pane.doc.text().contains("\"users\""), "the record passes: some user is bob");
    // A second constraint on the same array goes into the same any().
    double_click_json_line(&mut app, 8);
    assert_eq!(app.editor.text(), "select(any(.users[]; .name == \"bob\" and .age == 27))");
}

#[test]
fn double_clicking_puts_the_select_in_front_of_a_plain_query() {
    let mut app = app_with(Some(SAMPLE), &["--filter", ".users[]"]);
    render(&mut app, 100, 24);
    double_click_json_line(&mut app, 4);
    assert_eq!(app.editor.text(), "select(any(.users[]; .age == 31)) | .users[]");
}

#[test]
fn double_clicking_a_value_outside_an_array_uses_its_full_path() {
    let mut app = app_with(Some(SAMPLE), &[]);
    render(&mut app, 100, 24);
    double_click_json_line(&mut app, 11);
    assert_eq!(app.editor.text(), "select(.count == 2)");
}

const RECORDS: &str = concat!(
    r#"{"name":"ann","age":31,"addr":{"city":"Gent"}}"#,
    "\n",
    r#"{"name":"bob","age":27,"addr":{"city":"Brugge"}}"#
);

// RECORDS pretty-printed: lines 0-6 are ann, 7-13 are bob (8 = name, 9 = age, 11 = city).
#[test]
fn double_clicking_filters_the_records_of_a_stream() {
    let mut app = app_with(Some(RECORDS), &[]);
    render(&mut app, 100, 24);
    double_click_json_line(&mut app, 11);
    assert_eq!(app.editor.text(), "select(.addr.city == \"Brugge\")");
    let results = app.results_pane.doc.text().to_string();
    assert!(results.contains("bob") && !results.contains("ann"), "{results}");
}

#[test]
fn a_select_goes_in_front_of_whatever_the_query_does() {
    let mut app = app_with(Some(RECORDS), &["--filter", ".name"]);
    render(&mut app, 100, 24);
    double_click_json_line(&mut app, 9);
    assert_eq!(app.editor.text(), "select(.age == 27) | .name");
    assert_eq!(app.results_pane.doc.text(), "\"bob\"");

    let mut app = app_with(Some(RECORDS), &["--filter", ".name | select(. != \"x\")"]);
    render(&mut app, 100, 24);
    double_click_json_line(&mut app, 9);
    assert_eq!(app.editor.text(), "select(.age == 27) | .name | select(. != \"x\")");
}

#[test]
fn a_first_select_takes_the_extra_condition_as_an_and() {
    let mut app = app_with(Some(RECORDS), &["--filter", "select(.addr.city == \"Brugge\") | .name"]);
    render(&mut app, 100, 24);
    double_click_json_line(&mut app, 9);
    assert_eq!(app.editor.text(), "select(.addr.city == \"Brugge\" and .age == 27) | .name");
    assert_eq!(app.results_pane.doc.text(), "\"bob\"");

    let mut app = app_with(Some(RECORDS), &["--filter", "select(.name == \"x\" or .age > 1) | .name"]);
    render(&mut app, 100, 24);
    double_click_json_line(&mut app, 9);
    assert_eq!(app.editor.text(), "select((.name == \"x\" or .age > 1) and .age == 27) | .name");
}

#[test]
fn two_clicks_in_one_array_of_a_record_must_hold_for_the_same_element() {
    let record = r#"{"id":1,"orders":[{"id":1,"total":5},{"id":2,"total":9}]}"#;
    let mut app = app_with(Some(record), &[]);
    render(&mut app, 100, 24);
    // Pretty lines: 8 = orders[1].id, 9 = orders[1].total.
    double_click_json_line(&mut app, 9);
    assert_eq!(app.editor.text(), "select(any(.orders[]; .total == 9))");
    double_click_json_line(&mut app, 8);
    assert_eq!(app.editor.text(), "select(any(.orders[]; .total == 9 and .id == 2))");
    assert!(app.results_pane.doc.text().contains("\"orders\""));
    // orders[0].id is 1: no single element has total 9, id 2 and id 1, so nothing passes.
    double_click_json_line(&mut app, 4);
    assert_eq!(app.editor.text(), "select(any(.orders[]; .total == 9 and .id == 2 and .id == 1))");
    assert_eq!(app.results_pane.doc.text(), "");
}

#[test]
fn a_single_click_or_a_click_on_a_container_does_not_touch_the_filter() {
    let mut app = app_with(Some(SAMPLE), &["--filter", "."]);
    render(&mut app, 100, 24);
    click_json_line(&mut app, 3);
    assert_eq!(app.editor.text(), ".");
    // Two clicks on different lines are not a double-click.
    click_json_line(&mut app, 4);
    assert_eq!(app.editor.text(), ".");
    double_click_json_line(&mut app, 1);
    assert_eq!(app.editor.text(), ".");
    assert!(app.status_message.starts_with("double-click a value"));
}

#[test]
fn a_slow_second_click_is_not_a_double_click() {
    let mut app = app_with(Some(SAMPLE), &["--filter", "."]);
    render(&mut app, 100, 24);
    click_json_line(&mut app, 3);
    std::thread::sleep(std::time::Duration::from_millis(500));
    click_json_line(&mut app, 3);
    assert_eq!(app.editor.text(), ".");
}

#[test]
fn double_clicking_the_result_pane_does_nothing() {
    let mut app = app_with(Some(SAMPLE), &["--filter", "."]);
    render(&mut app, 100, 24);
    let (x, y) = (app.rects.results.x + 3, app.rects.results.y + 4);
    for _ in 0..2 {
        mouse(&mut app, MouseEventKind::Down(MouseButton::Left), x, y);
    }
    assert_eq!(app.editor.text(), ".");
}

fn status_row(app: &mut jqplay::app::App, width: u16) -> String {
    // The test backend prints every row between quotes.
    render(app, width, 16).lines().last().unwrap().trim_matches('"').to_string()
}

#[test]
fn the_result_count_sits_at_the_right_end_of_the_status_bar() {
    let mut app = app_with(Some(SAMPLE), &["--filter", ".users[]"]);
    let row = status_row(&mut app, 120);
    assert!(row.starts_with(" ● json:valid (1)"), "{row}");
    assert!(row.trim_end().ends_with("result:2"), "{row}");
    assert_eq!(row.chars().count(), 120);
    // A lone array counts its items, like the input does.
    let mut app = app_with(Some(SAMPLE), &["--filter", ".users"]);
    assert!(status_row(&mut app, 120).trim_end().ends_with("result:2"));
    let mut app = app_with(Some(SAMPLE), &["--filter", ".users[] | select(.age > 99)"]);
    assert!(status_row(&mut app, 120).trim_end().ends_with("result:0"));
}

#[test]
fn raw_output_counts_lines_and_says_so() {
    let mut app = app_with(Some(SAMPLE), &["--filter", ".users[].name", "--raw"]);
    assert!(status_row(&mut app, 120).trim_end().ends_with("result:2 lines"));
    let mut app = app_with(Some(SAMPLE), &["--filter", ".count", "--raw"]);
    assert!(status_row(&mut app, 120).trim_end().ends_with("result:1 line"));
}

#[test]
fn there_is_no_result_count_without_results() {
    for (json, filter) in [(SAMPLE, ""), (SAMPLE, ".users[] | error"), ("{oops", ".")] {
        let mut app = app_with(Some(json), &["--filter", filter]);
        assert!(!status_row(&mut app, 120).contains("result:"), "{json} {filter}");
    }
}

#[test]
fn the_result_count_gives_way_to_nothing_but_the_key_hints() {
    let mut app = app_with(Some(SAMPLE), &["--filter", ".users[]"]);
    let row = status_row(&mut app, 70);
    assert!(row.contains("Color") && row.trim_end().ends_with("result:2"), "{row}");
    assert_eq!(row.chars().count(), 70);
    // Too narrow to keep both: the count is dropped, not the options.
    let row = status_row(&mut app, 30);
    assert!(!row.contains("result:"), "{row}");
}

#[test]
fn double_clicking_leaves_the_status_message_alone() {
    let mut app = app_with(Some(SAMPLE), &[]);
    render(&mut app, 100, 24);
    double_click_json_line(&mut app, 7);
    assert_eq!(app.status_message, "");
}

// Left-clicks the filter pane's text area, `col` columns and `row` rows from its top left (inside the border).
fn click_filter(app: &mut jqplay::app::App, col: u16, row: u16) {
    let (x, y) = (app.rects.filter.x + 1 + col, app.rects.filter.y + 1 + row);
    mouse(app, MouseEventKind::Down(MouseButton::Left), x, y);
    mouse(app, MouseEventKind::Up(MouseButton::Left), x, y);
}

#[test]
fn clicking_the_filter_text_puts_the_cursor_there_and_focuses_the_pane() {
    let mut app = app_with(Some(SAMPLE), &["--filter", ".users[].name"]);
    focus_next(&mut app);
    render(&mut app, 80, 20);
    assert_eq!(app.focus, Focus::Json);
    // Two columns of "> " in front of the text.
    click_filter(&mut app, 2 + 3, 0);
    assert_eq!(app.focus, Focus::Filter);
    assert_eq!(app.editor.cursor(), 3);
    type_text(&mut app, "X");
    assert_eq!(app.editor.text(), ".usXers[].name");
}

#[test]
fn clicks_left_of_the_text_or_past_its_end_clamp() {
    let mut app = app_with(Some(SAMPLE), &["--filter", ".count"]);
    render(&mut app, 80, 20);
    click_filter(&mut app, 0, 0);
    assert_eq!(app.editor.cursor(), 0);
    click_filter(&mut app, 60, 0);
    assert_eq!(app.editor.cursor(), 6);
    // Below the last line stays on the last line.
    click_filter(&mut app, 4, 0);
    assert_eq!(app.editor.cursor(), 2);
}

#[test]
fn clicking_a_line_of_a_multi_line_filter() {
    let mut app = app_with(Some(SAMPLE), &["--filter", ".users\n| .name"]);
    render(&mut app, 80, 20);
    click_filter(&mut app, 2 + 3, 1);
    assert_eq!(app.editor.cursor(), ".users\n".chars().count() + 3);
    click_filter(&mut app, 2 + 1, 0);
    assert_eq!(app.editor.cursor(), 1);
}

#[test]
fn clicking_a_horizontally_scrolled_filter_maps_to_the_visible_column() {
    let long = format!(".{}", "a".repeat(40));
    let mut app = app_with(Some(SAMPLE), &["--filter", &long]);
    // 20 columns wide: 18 inside the border. The cursor at the end (41) scrolls the text by 41 + 3 - 18 = 26,
    // so the fifth text cell shows character 26 + 4 - 2 (the "> " prompt scrolled out of view as well).
    render(&mut app, 20, 20);
    click_filter(&mut app, 4, 0);
    assert_eq!(app.editor.cursor(), 26 + 4 - 2);
}

#[test]
fn clicking_the_filter_border_only_focuses_and_closes_the_completion_popup() {
    let mut app = app_with(Some(SAMPLE), &["--filter", ".us"]);
    render(&mut app, 80, 20);
    type_text(&mut app, "e");
    assert!(app.completion.open);
    let before = app.editor.cursor();
    let (x, y) = (app.rects.filter.x + 3, app.rects.filter.y);
    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), x, y);
    assert_eq!(app.editor.cursor(), before);
    assert!(!app.completion.open);
}

// RECORDS slurped (or a plain array) pretty-printed: lines 1-7 are ann, 8-14 are bob (10 = age, 12 = city).
#[test]
fn the_elements_of_a_slurped_stream_are_filtered_like_records() {
    let mut app = app_with(Some(RECORDS), &["--slurp"]);
    render(&mut app, 100, 24);
    double_click_json_line(&mut app, 10);
    assert_eq!(app.editor.text(), ".[] | select(.age == 27)");
    let results = app.results_pane.doc.text().to_string();
    assert!(results.contains("bob") && !results.contains("ann"), "{results}");
}

#[test]
fn a_leading_iteration_of_the_query_is_kept_for_a_root_array() {
    let array = r#"[{"name":"ann","age":31,"addr":{"city":"Gent"}},{"name":"bob","age":27,"addr":{"city":"Brugge"}}]"#;
    let mut app = app_with(Some(array), &["--filter", ".[] | .name"]);
    render(&mut app, 100, 24);
    double_click_json_line(&mut app, 10);
    assert_eq!(app.editor.text(), ".[] | select(.age == 27) | .name");
    assert_eq!(app.results_pane.doc.text(), "\"bob\"");
    double_click_json_line(&mut app, 12);
    assert_eq!(app.editor.text(), ".[] | select(.age == 27 and .addr.city == \"Brugge\") | .name");
    assert_eq!(app.results_pane.doc.text(), "\"bob\"");
}

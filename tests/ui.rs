mod support;

use jqplay::app::Outcome;
use ratatui::crossterm::event::{KeyCode, KeyModifiers};
use support::{SAMPLE, app_with, ctrl, flush_debounce, press, render, type_text};

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
fn tab_opens_completion_and_enter_accepts() {
    let mut app = app_with(Some(SAMPLE), &[]);
    type_text(&mut app, ".us");
    press(&mut app, KeyCode::Tab, KeyModifiers::NONE);
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
    press(&mut app, KeyCode::Tab, KeyModifiers::NONE);
    type_text(&mut app, " ");
    assert!(!app.completion.open);
    assert_eq!(app.editor.text(), ".us ");
}

#[test]
fn toggles_flip_status_and_output() {
    let mut app = app_with(Some(SAMPLE), &["--filter", ".users[0].name"]);
    assert!(render(&mut app, 100, 12).lines().any(|l| l.contains("\"ann\"")));
    ctrl(&mut app, 'n');
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
    ctrl(&mut app, 'n');
    type_text(&mut app, "rcsSC");
    assert_eq!(
        app.options,
        jqplay::jq::Options { raw: true, compact: true, sort_keys: true, slurp: true, color: true }
    );
    ctrl(&mut app, 'n');
    type_text(&mut app, "r");
    assert!(!app.options.raw);
}

#[test]
fn old_ctrl_toggles_are_gone() {
    let mut app = app_with(Some(SAMPLE), &[]);
    ctrl(&mut app, 'n');
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
    ctrl(&mut app, 'n');
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
    ctrl(&mut app, 'n');
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
    ctrl(&mut app, 'b');
    ctrl(&mut app, 'b');
    press(&mut app, KeyCode::End, KeyModifiers::NONE);
    assert!(render(&mut app, 100, 12).contains("199"));
}

#[test]
fn esc_dismisses_status_message_and_popup() {
    let mut app = app_with(Some(SAMPLE), &[]);
    ctrl(&mut app, 'n');
    ctrl(&mut app, 'y');
    assert!(!app.status_message.is_empty());
    press(&mut app, KeyCode::Esc, KeyModifiers::NONE);
    assert!(app.status_message.is_empty());
    ctrl(&mut app, 'b');
    type_text(&mut app, ".us");
    press(&mut app, KeyCode::Tab, KeyModifiers::NONE);
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
    assert!(last.contains("json:valid") && last.contains("color [C]") && last.contains('…'), "{last}");
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

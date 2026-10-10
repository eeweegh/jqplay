#![allow(dead_code)]
use clap::Parser;
use jqplay::app::{App, Event};
use jqplay::cli::Cli;
use jqplay::config::{Config, Settings};
use jqplay::job::execute;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::crossterm::event::{Event as TermEvent, KeyCode, KeyEvent, KeyModifiers};

pub const SAMPLE: &str = r#"{"users":[{"name":"ann","age":31},{"name":"bob","age":27}],"count":2}"#;

pub fn app_with(json: Option<&str>, args: &[&str]) -> App {
    let cli = Cli::parse_from(
        ["jqplay", "--history-file", "", "--no-color"].into_iter().chain(args.iter().copied()),
    );
    let settings = Settings::resolve(&cli, &Config::default());
    let mut app = App::new(&settings, json.map(String::from));
    settle(&mut app);
    app
}

// Runs pending jq jobs to completion, standing in for the event loop's worker threads.
pub fn settle(app: &mut App) {
    loop {
        let jobs = app.take_jobs();
        if jobs.is_empty() {
            return;
        }
        for job in jobs {
            app.handle(Event::Reply(execute(job)));
        }
    }
}

pub fn press(app: &mut App, code: KeyCode, modifiers: KeyModifiers) {
    app.handle(Event::Term(TermEvent::Key(KeyEvent::new(code, modifiers))));
    settle(app);
}

pub fn ctrl(app: &mut App, c: char) {
    press(app, KeyCode::Char(c), KeyModifiers::CONTROL);
}

pub fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        press(app, KeyCode::Char(c), KeyModifiers::NONE);
    }
}

// Fires the debounce timer immediately, as if the typing pause had elapsed.
pub fn flush_debounce(app: &mut App) {
    if let Some(deadline) = app.deadline() {
        app.tick(deadline);
        settle(app);
    }
}

pub fn render(app: &mut App, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| jqplay::ui::draw(f, app)).unwrap();
    terminal.backend().to_string()
}

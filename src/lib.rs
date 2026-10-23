//! jqplay: a terminal UI for exploring JSON with jq.

#[cfg(not(unix))]
compile_error!("jqplay needs a Unix terminal: it uses termios, /dev/tty and job-control signals");

mod ansi;
pub mod app;
pub mod cli;
mod completion;
pub mod config;
mod editor;
mod error;
pub mod generate;
pub mod history;
mod input;
pub mod job;
pub mod jq;
mod paths;
mod run;
mod terminal;
mod theme;
pub mod ui;
mod util;

use app::{App, Outcome};
use cli::Cli;
use config::{Config, Settings};
pub use error::Error;

pub fn run(cli: &Cli) -> Result<Outcome, Error> {
    if !jq::is_available() {
        return Err(Error::JqMissing);
    }
    let config = Config::load(cli.config.as_deref())?;
    let settings = Settings::resolve(cli, &config);
    let json = input::load_initial_json(settings.file.as_deref())?;
    if json.as_deref().is_some_and(|j| j.trim().is_empty()) {
        return Ok(Outcome::NoInput);
    }
    let settings = settings.with_input_size(json.as_deref().map_or(0, |j| j.chars().count()));
    let mut app = App::new(&settings, json);
    let outcome = run::event_loop(&mut app, settings.mouse);
    // Only a normal quit (accept) saves the history, whether or not the filter ran.
    if matches!(outcome, Ok(Outcome::Clean(_))) {
        app.save_history();
    }
    Ok(outcome?)
}

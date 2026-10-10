//! jqplay: a terminal UI for exploring JSON with jq.

pub mod ansi;
pub mod app;
pub mod cli;
pub mod completion;
pub mod config;
pub mod editor;
pub mod error;
pub mod generate;
pub mod history;
pub mod input;
pub mod job;
pub mod jq;
pub mod paths;
pub mod run;
pub mod terminal;
pub mod theme;
pub mod ui;

use app::{App, Outcome};
use cli::Cli;
use config::{Config, Settings};
use error::Error;

pub fn run(cli: &Cli) -> anyhow::Result<Outcome> {
    if !jq::is_available() {
        return Err(Error::JqMissing.into());
    }
    let config = Config::load(cli.config.as_deref())?;
    let settings = Settings::resolve(cli, &config);
    let json = input::load_initial_json(settings.file.as_deref())?;
    let settings = settings.with_input_size(json.as_deref().map_or(0, |j| j.chars().count()));
    let mut app = App::new(&settings, json);
    Ok(run::event_loop(&mut app, settings.mouse)?)
}

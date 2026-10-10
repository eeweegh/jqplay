use crate::cli::Cli;
use crate::error::Error;
use crate::jq::Options;
use crate::paths::{DEFAULT_HISTORY_FILE, default_config_file, expand_tilde};
use crate::theme::ThemeName;
use serde::Deserialize;
use std::path::{Path, PathBuf};

// Inputs above this many chars start with color off; Ctrl+G still overrides live.
pub const LARGE_INPUT_THRESHOLD: usize = 200_000;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub theme: Option<ThemeName>,
    pub raw: Option<bool>,
    pub compact: Option<bool>,
    pub sort_keys: Option<bool>,
    pub slurp: Option<bool>,
    pub color: Option<bool>,
    pub history_file: Option<String>,
    pub mouse: Option<bool>,
    pub autopair: Option<bool>,
    pub autocomplete: Option<bool>,
}

impl Config {
    // A missing default file is fine; a missing explicit --config path is an error.
    pub fn load(explicit: Option<&Path>) -> Result<Self, Error> {
        let (path, required) = match explicit {
            Some(p) => (p.to_path_buf(), true),
            None => match default_config_file() {
                Some(p) => (p, false),
                None => return Ok(Self::default()),
            },
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => toml::from_str(&text).map_err(|e| Error::Config { path, message: e.to_string() }),
            Err(e) if !required && e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(Error::Config { path, message: e.to_string() }),
        }
    }
}

// Effective startup settings: CLI flags over the config file over built-in defaults.
#[derive(Debug)]
pub struct Settings {
    pub options: Options,
    pub explicit_color: Option<bool>,
    pub theme: ThemeName,
    pub history_file: Option<PathBuf>,
    pub mouse: bool,
    pub autopair: bool,
    pub autocomplete: bool,
    pub filter: Option<String>,
    pub file: Option<PathBuf>,
}

impl Settings {
    pub fn resolve(cli: &Cli, config: &Config) -> Self {
        let flag = |cli_on: bool, cfg: Option<bool>| cli_on || cfg.unwrap_or(false);
        let explicit_color = if cli.color {
            Some(true)
        } else if cli.no_color {
            Some(false)
        } else {
            config.color
        };
        let history = cli
            .history_file
            .clone()
            .or_else(|| config.history_file.clone())
            .unwrap_or_else(|| DEFAULT_HISTORY_FILE.to_string());
        Self {
            options: Options {
                raw: flag(cli.raw, config.raw),
                compact: flag(cli.compact, config.compact),
                sort_keys: flag(cli.sort_keys, config.sort_keys),
                slurp: flag(cli.slurp, config.slurp),
                color: explicit_color.unwrap_or(true),
            },
            explicit_color,
            theme: cli.theme.or(config.theme).unwrap_or_default(),
            history_file: (!history.is_empty()).then(|| expand_tilde(&history)),
            mouse: !cli.no_mouse && config.mouse.unwrap_or(true),
            autopair: config.autopair.unwrap_or(true),
            autocomplete: config.autocomplete.unwrap_or(true),
            filter: cli.filter.clone(),
            file: cli.file.clone(),
        }
    }

    // Applies the large-input color default once the input size is known.
    #[must_use]
    pub fn with_input_size(mut self, chars: usize) -> Self {
        if self.explicit_color.is_none() && chars > LARGE_INPUT_THRESHOLD {
            self.options.color = false;
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    fn cli(args: &[&str]) -> Cli {
        Cli::parse_from(std::iter::once("jqplay").chain(args.iter().copied()))
    }

    #[test]
    fn cli_overrides_config_overrides_defaults() {
        let cfg: Config =
            toml::from_str("theme = \"mono\"\nraw = true\nmouse = false\ncolor = false").unwrap();
        let s = Settings::resolve(&cli(&["--theme", "monokai"]), &cfg);
        assert_eq!(s.theme, ThemeName::Monokai);
        assert!(s.options.raw && !s.options.color && !s.mouse);
        let s = Settings::resolve(&cli(&["--color"]), &cfg);
        assert!(s.options.color);
    }

    #[test]
    fn large_input_turns_color_off_unless_explicit() {
        let s = Settings::resolve(&cli(&[]), &Config::default()).with_input_size(LARGE_INPUT_THRESHOLD + 1);
        assert!(!s.options.color);
        let s = Settings::resolve(&cli(&["--color"]), &Config::default())
            .with_input_size(LARGE_INPUT_THRESHOLD + 1);
        assert!(s.options.color);
    }

    #[test]
    fn autopair_defaults_on_and_can_be_disabled() {
        assert!(Settings::resolve(&cli(&[]), &Config::default()).autopair);
        let cfg: Config = toml::from_str("autopair = false").unwrap();
        assert!(!Settings::resolve(&cli(&[]), &cfg).autopair);
        assert!(Settings::resolve(&cli(&[]), &Config::default()).autocomplete);
        let cfg: Config = toml::from_str("autocomplete = false").unwrap();
        assert!(!Settings::resolve(&cli(&[]), &cfg).autocomplete);
    }

    #[test]
    fn empty_history_file_disables_history() {
        let s = Settings::resolve(&cli(&["--history-file", ""]), &Config::default());
        assert!(s.history_file.is_none());
    }

    #[test]
    fn unknown_config_keys_are_rejected() {
        assert!(toml::from_str::<Config>("bogus = 1").is_err());
    }

    #[test]
    fn color_flags_conflict() {
        assert!(Cli::try_parse_from(["jqplay", "--color", "--no-color"]).is_err());
    }
}

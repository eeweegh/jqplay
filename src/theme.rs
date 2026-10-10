use clap::ValueEnum;
use ratatui::style::{Color, Modifier, Style};
use serde::Deserialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, ValueEnum, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeName {
    #[default]
    SolarizedDark,
    SolarizedLight,
    Monokai,
    Mono,
}

// UI chrome only; JSON colors always come from jq's own output.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    pub border: Style,
    pub border_focus: Style,
    pub text: Style,
    pub status: Style,
    pub error: Style,
    pub selected: Style,
    pub option_on: Style,
    pub option_off: Style,
    pub working: Style,
    pub idle: Style,
}

impl Theme {
    pub fn named(name: ThemeName) -> Self {
        let fg = |c| Style::default().fg(c);
        let dark = |accent| Self {
            border: fg(Color::DarkGray),
            border_focus: fg(accent),
            text: fg(Color::Gray),
            status: fg(Color::Gray),
            error: fg(Color::Red),
            selected: Style::default().fg(Color::Black).bg(accent),
            option_on: fg(Color::Gray).add_modifier(Modifier::BOLD),
            option_off: fg(Color::Gray).add_modifier(Modifier::DIM),
            working: fg(Color::Red),
            idle: fg(Color::Green),
        };
        match name {
            ThemeName::SolarizedDark | ThemeName::Monokai => dark(Color::LightYellow),
            ThemeName::SolarizedLight => Self {
                border_focus: fg(Color::Yellow),
                text: fg(Color::Black),
                status: fg(Color::Black),
                selected: Style::default().fg(Color::White).bg(Color::Yellow),
                option_on: fg(Color::Black).add_modifier(Modifier::BOLD),
                option_off: fg(Color::Black).add_modifier(Modifier::DIM),
                ..dark(Color::Yellow)
            },
            ThemeName::Mono => {
                let plain = Style::default();
                let reversed = plain.add_modifier(Modifier::REVERSED);
                Self {
                    border: plain,
                    border_focus: reversed,
                    text: plain,
                    status: plain,
                    error: reversed,
                    selected: reversed,
                    option_on: plain.add_modifier(Modifier::BOLD),
                    option_off: plain.add_modifier(Modifier::DIM),
                    working: reversed,
                    idle: plain,
                }
            }
        }
    }
}

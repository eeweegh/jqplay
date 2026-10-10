use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
};
use ratatui::crossterm::execute;
use std::io::stdout;

// Owns the terminal modes; raw mode already clears IXON, so Ctrl+Q/Ctrl+S reach the app.
#[derive(Debug)]
pub struct Tui {
    pub terminal: DefaultTerminal,
    mouse: bool,
}

fn leave_extras(mouse: bool) {
    let _ = execute!(stdout(), DisableBracketedPaste);
    if mouse {
        let _ = execute!(stdout(), DisableMouseCapture);
    }
}

impl Tui {
    pub fn start(mouse: bool) -> std::io::Result<Self> {
        let terminal = ratatui::try_init()?;
        execute!(stdout(), EnableBracketedPaste)?;
        if mouse {
            execute!(stdout(), EnableMouseCapture)?;
        }
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            leave_extras(mouse);
            previous(info);
        }));
        Ok(Self { terminal, mouse })
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        leave_extras(self.mouse);
        ratatui::restore();
    }
}

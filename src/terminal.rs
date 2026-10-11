use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::execute;
use std::io::stdout;

// Owns the terminal modes; raw mode already clears IXON, so Ctrl+Q/Ctrl+S reach the app.
#[derive(Debug)]
pub struct Tui {
    pub terminal: DefaultTerminal,
    mouse: bool,
    kitty: bool,
}

// Terminals known to speak the kitty keyboard protocol, which makes Shift+Enter distinguishable. Detected
// from the environment: asking the terminal would stall startup where nobody answers.
fn kitty_keyboard_terminal() -> bool {
    let program = std::env::var("TERM_PROGRAM").unwrap_or_default();
    ["WezTerm", "ghostty", "iTerm.app"].contains(&program.as_str())
        || std::env::var("TERM").is_ok_and(|t| t == "xterm-kitty")
}

fn leave_extras(mouse: bool, kitty: bool) {
    if kitty {
        let _ = execute!(stdout(), PopKeyboardEnhancementFlags);
    }
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
        let kitty = kitty_keyboard_terminal();
        if kitty {
            execute!(
                stdout(),
                PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
            )?;
        }
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            leave_extras(mouse, kitty);
            previous(info);
        }));
        Ok(Self { terminal, mouse, kitty })
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        leave_extras(self.mouse, self.kitty);
        ratatui::restore();
    }
}

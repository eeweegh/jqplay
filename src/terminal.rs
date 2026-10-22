use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{
    DisableBracketedPaste, DisableMouseCapture, EnableBracketedPaste, EnableMouseCapture,
    KeyboardEnhancementFlags, PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{EnterAlternateScreen, enable_raw_mode};
use ratatui::layout::Rect;
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

fn enter_extras(mouse: bool, kitty: bool) -> std::io::Result<()> {
    execute!(stdout(), EnableBracketedPaste)?;
    if mouse {
        execute!(stdout(), EnableMouseCapture)?;
    }
    if kitty {
        execute!(
            stdout(),
            PushKeyboardEnhancementFlags(KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES)
        )?;
    }
    Ok(())
}

impl Tui {
    pub fn start(mouse: bool) -> std::io::Result<Self> {
        let terminal = ratatui::try_init()?;
        let kitty = kitty_keyboard_terminal();
        enter_extras(mouse, kitty)?;
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            leave_extras(mouse, kitty);
            previous(info);
        }));
        Ok(Self { terminal, mouse, kitty })
    }

    // Like the tty driver's Ctrl+Z: hand the terminal back, stop the whole foreground process group, and
    // take the terminal over again (with a full repaint) once the shell continues us.
    pub fn suspend(&mut self) -> std::io::Result<()> {
        leave_extras(self.mouse, self.kitty);
        ratatui::restore();
        rustix::process::kill_current_process_group(rustix::process::Signal::TSTP)?;
        // Not ratatui::try_init: it queries the cursor position, and the input thread would eat the reply.
        enable_raw_mode()?;
        execute!(stdout(), EnterAlternateScreen)?;
        enter_extras(self.mouse, self.kitty)?;
        // Not Terminal::clear (it queries the cursor, which the input thread would answer for); resizing to
        // the current size clears the screen and forces a full repaint, and picks up a changed window size.
        let size = self.terminal.size()?;
        self.terminal.resize(Rect::from(size))
    }
}

impl Drop for Tui {
    fn drop(&mut self) {
        leave_extras(self.mouse, self.kitty);
        ratatui::restore();
    }
}

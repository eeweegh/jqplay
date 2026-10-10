use crate::error::Error;
use std::fs::OpenOptions;
use std::io::{IsTerminal, Read};
use std::os::fd::AsFd;
use std::path::Path;

// Reads the JSON from the file, or from piped stdin (then points fd 0 back at the terminal).
pub fn load_initial_json(file: Option<&Path>) -> Result<Option<String>, Error> {
    if let Some(path) = file {
        return std::fs::read_to_string(path)
            .map(Some)
            .map_err(|source| Error::ReadInput { path: path.to_path_buf(), source });
    }
    let stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Ok(None);
    }
    let mut data = String::new();
    stdin.lock().read_to_string(&mut data).map_err(Error::ReadStdin)?;
    reattach_stdin_to_tty();
    Ok(Some(data))
}

// Prefers the real device path of stdout/stderr: kqueue (macOS) cannot poll the /dev/tty alias.
fn tty_path() -> String {
    [std::io::stderr().as_fd(), std::io::stdout().as_fd()]
        .into_iter()
        .filter(|fd| rustix::termios::isatty(fd))
        .find_map(|fd| rustix::termios::ttyname(fd, Vec::new()).ok())
        .map_or_else(|| "/dev/tty".to_string(), |name| name.to_string_lossy().into_owned())
}

fn reattach_stdin_to_tty() {
    if let Ok(tty) = OpenOptions::new().read(true).write(true).open(tty_path()) {
        let _ = rustix::stdio::dup2_stdin(&tty);
    }
}

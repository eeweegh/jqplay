use crate::app::{App, Event, Outcome};
use crate::job::{Job, execute};
use crate::terminal::Tui;
use crate::ui;
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::thread;
use std::time::Instant;

fn spawn_job(job: Job, tx: &Sender<Event>) {
    let tx = tx.clone();
    thread::spawn(move || {
        let _ = tx.send(Event::Reply(execute(job)));
    });
}

fn spawn_input(tx: Sender<Event>) {
    thread::spawn(move || {
        while let Ok(event) = ratatui::crossterm::event::read() {
            if tx.send(Event::Term(event)).is_err() {
                break;
            }
        }
    });
}

pub fn event_loop(app: &mut App, mouse: bool) -> std::io::Result<Outcome> {
    let mut tui = Tui::start(mouse)?;
    let (tx, rx) = mpsc::channel();
    spawn_input(tx.clone());
    loop {
        for job in app.take_jobs() {
            spawn_job(job, &tx);
        }
        tui.terminal.draw(|frame| ui::draw(frame, app))?;
        let first = match app.deadline() {
            Some(deadline) => rx.recv_timeout(deadline.saturating_duration_since(Instant::now())),
            None => rx.recv().map_err(|_| RecvTimeoutError::Disconnected),
        };
        match first {
            Ok(event) => {
                app.handle(event);
                while let Ok(event) = rx.try_recv() {
                    app.handle(event);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => return Ok(Outcome::Abort),
        }
        app.tick(Instant::now());
        if let Some(outcome) = app.outcome() {
            return Ok(outcome.clone());
        }
    }
}

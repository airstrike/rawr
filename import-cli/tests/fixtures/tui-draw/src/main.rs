//! `examples/tui`'s marked `app.rs` and `ui.rs`, driven headless: for each
//! line of stdin, its keys go to `app::update`, `ui::draw` draws one frame
//! on ratatui's `TestBackend` with the native `&mut Frame` lent to it
//! (D558), and the frame's buffer is printed with its styles. The test
//! copies the two marked files in from the example over the copies here.

mod app;
mod ui;

use std::io::BufRead as _;

use ratatui::backend::{Backend as _, TestBackend};
use ratatui::Terminal;

fn main() {
    let mut app = app::App::new();
    let mut terminal = Terminal::new(TestBackend::new(40, 10)).expect("a test backend starts");
    for line in std::io::stdin().lock().lines() {
        let line = line.expect("stdin is text");
        for word in line.split_whitespace() {
            app::update(&mut app, key(word));
        }
        terminal
            .draw(|frame| ui::draw(frame, &mut app))
            .expect("a test backend draws");
        let backend = terminal.backend_mut();
        let buffer = backend.buffer().clone();
        for y in 0..buffer.area.height {
            let row: String = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            println!("|{row}|");
        }
        for line in format!("{buffer:?}").lines() {
            println!("buffer {line}");
        }
        println!("cursor {:?}", backend.get_cursor_position().ok());
        println!("end");
    }
}

/// A word of stdin as a key: `up`, `down`, `enter`, `esc`, `back`,
/// `space`, or a character.
fn key(word: &str) -> app::Key {
    match word {
        "up" => app::Key::Up,
        "down" => app::Key::Down,
        "enter" => app::Key::Enter,
        "esc" => app::Key::Esc,
        "back" => app::Key::Backspace,
        "space" => app::Key::Char(' '),
        word => app::Key::Char(word.chars().next().unwrap_or(' ')),
    }
}

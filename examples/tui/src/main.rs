//! A to-do list in the terminal, written as an ordinary ratatui app.
//!
//! `main` is native Rust. It owns the terminal, the event loop and the
//! keys. `app.rs` and `ui.rs`, which start with `// rawr`, hold what the
//! keys do and how the screen is drawn: `ui::draw` takes ratatui's `&mut
//! Frame`, as a native one would. `cargo run` builds them natively. Under
//! `rawr run`, a save of either shows on the screen at once, and the list
//! and the selection stay as they were.
//!
//! Keys: `a` adds a to-do, Enter saves it and Esc cancels it. Up and Down
//! move, Space checks the selected to-do off, `d` deletes it, and `q` or
//! Ctrl-C quits.

mod app;
mod ui;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;

fn main() -> std::io::Result<()> {
    let terminal = ratatui::init();
    let result = run(terminal);
    ratatui::restore();
    result
}

fn run(mut terminal: DefaultTerminal) -> std::io::Result<()> {
    let mut app = app::App::new();
    loop {
        terminal.draw(|frame| ui::draw(frame, &mut app))?;
        // A resize, or anything else, draws again.
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        if quits(&key, &app) {
            return Ok(());
        }
        if let Some(key) = translate(key.code) {
            app::update(&mut app, key);
        }
    }
}

/// Ctrl-C quits, and so does `q` when no to-do is being written.
fn quits(key: &KeyEvent, app: &app::App) -> bool {
    let control_c = key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL);
    control_c || (key.code == KeyCode::Char('q') && !app.editing)
}

/// The keys the app knows, as its own type.
fn translate(code: KeyCode) -> Option<app::Key> {
    match code {
        KeyCode::Char(char) => Some(app::Key::Char(char)),
        KeyCode::Backspace => Some(app::Key::Backspace),
        KeyCode::Enter => Some(app::Key::Enter),
        KeyCode::Esc => Some(app::Key::Esc),
        KeyCode::Up => Some(app::Key::Up),
        KeyCode::Down => Some(app::Key::Down),
        _ => None,
    }
}

//! A to-do list in the terminal, written as an ordinary ratatui app.
//!
//! `main` is native Rust. It owns the terminal, the event loop, the keys,
//! the drawing and the list's selection. `app.rs`, which starts with
//! `// rawr`, holds what the keys do and what the screen says. `cargo run`
//! builds both natively. Under `rawr run`, a save of `app.rs` shows on the
//! screen at once, and the list and the selection stay as they were.
//!
//! Keys: `a` adds a to-do, Enter saves it and Esc cancels it. Up and Down
//! move, Space checks the selected to-do off, `d` deletes it, and `q` or
//! Ctrl-C quits.

mod app;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout, Position};
use ratatui::style::{Color, Modifier, Style, Stylize as _};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph};
use ratatui::{DefaultTerminal, Frame};

fn main() -> std::io::Result<()> {
    let terminal = ratatui::init();
    let result = run(terminal);
    ratatui::restore();
    result
}

fn run(mut terminal: DefaultTerminal) -> std::io::Result<()> {
    let mut app = app::App::new();
    let mut list = ListState::default().with_selected(Some(0));
    loop {
        let view = app::view(&app, terminal.size()?.width);
        terminal.draw(|frame| draw(frame, &view, &mut list))?;
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
        match key.code {
            KeyCode::Up => list.select_previous(),
            KeyCode::Down => list.select_next(),
            code => {
                if let Some(key) = translate(code) {
                    app::update(&mut app, key, list.selected());
                }
            }
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
        _ => None,
    }
}

fn draw(frame: &mut Frame, view: &app::View, list: &mut ListState) {
    let [input, items, help] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    let style = if view.editing {
        Style::new().fg(Color::Yellow)
    } else {
        Style::new().dim()
    };
    frame.render_widget(
        Paragraph::new(view.input.as_str())
            .style(style)
            .block(Block::bordered().title(view.prompt.as_str())),
        input,
    );
    if view.editing {
        frame.set_cursor_position(Position::new(input.x + 1 + view.cursor, input.y + 1));
    }

    let rows: Vec<ListItem> = view
        .rows
        .iter()
        .map(|row| {
            let (mark, style) = if row.done {
                (
                    "✓ ",
                    Style::new()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::CROSSED_OUT),
                )
            } else {
                ("☐ ", Style::new())
            };
            ListItem::new(Line::from(vec![
                Span::raw(mark),
                Span::styled(row.text.as_str(), style),
            ]))
        })
        .collect();
    frame.render_stateful_widget(
        List::new(rows)
            .block(Block::bordered().title(view.title.as_str()))
            .highlight_style(Style::new().add_modifier(Modifier::REVERSED))
            .highlight_symbol("> "),
        items,
        list,
    );

    frame.render_widget(Line::from(view.help.as_str()).dim(), help);
}

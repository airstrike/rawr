//! A ratatui program whose `main` is native and whose view model is marked
//! (D534). At a terminal it runs as any ratatui app does. With
//! `TUI_HEADLESS` set, it draws one frame on a `TestBackend` for each line
//! of stdin and prints the frame's rows.

mod app;

use std::io::BufRead as _;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::layout::{Constraint, Layout};
use ratatui::widgets::{Block, List, ListState, Paragraph};
use ratatui::Frame;

fn draw(frame: &mut Frame, view: app::View, list: &mut ListState) {
    let [top, bottom] =
        Layout::vertical([Constraint::Min(3), Constraint::Length(3)]).areas(frame.area());
    let items = List::new(view.items)
        .block(Block::bordered().title(view.title))
        .highlight_symbol("> ");
    frame.render_stateful_widget(items, top, list);
    frame.render_widget(Paragraph::new(view.status).block(Block::bordered()), bottom);
}

fn headless(app: &mut app::App, list: &mut ListState) -> std::io::Result<()> {
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(24, 8))
        .expect("a test backend starts");
    for line in std::io::stdin().lock().lines() {
        match line?.as_str() {
            "down" => list.select_next(),
            text => text.chars().for_each(|key| app::typed(app, key)),
        }
        let view = app::view(app);
        terminal
            .draw(|frame| draw(frame, view, list))
            .expect("a test backend draws");
        let buffer = terminal.backend().buffer();
        for y in 0..buffer.area.height {
            let row: String = (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect();
            println!("|{row}|");
        }
        println!("end");
    }
    Ok(())
}

fn main() -> std::io::Result<()> {
    let mut app = app::App::new();
    let mut list = ListState::default().with_selected(Some(0));
    if std::env::var_os("TUI_HEADLESS").is_some() {
        return headless(&mut app, &mut list);
    }
    let mut terminal = ratatui::init();
    loop {
        let view = app::view(&app);
        terminal.draw(|frame| draw(frame, view, &mut list))?;
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        match key.code {
            KeyCode::Char('q') => break,
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => break,
            KeyCode::Char('!') => panic!("asked to panic"),
            KeyCode::Down => list.select_next(),
            KeyCode::Up => list.select_previous(),
            KeyCode::Char(key) => app::typed(&mut app, key),
            _ => {}
        }
    }
    ratatui::restore();
    Ok(())
}

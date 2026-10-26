// rawr
//! What the screen shows: `draw` renders the app on ratatui's frame, as any
//! ratatui app does. Under `rawr run` this file is interpreted, and a save
//! repaints the screen at once, with the list and the selection kept.
//! `cargo build` compiles it like any other module.

use ratatui::layout::{Constraint, Layout, Margin, Position};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, List, ListItem, ListState, Paragraph};
use ratatui::Frame;

use crate::app::App;

/// Draw `app` on `frame`.
pub fn draw(frame: &mut Frame, app: &mut App) {
    let areas = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(frame.area());
    let (input, items, help) = (areas[0], areas[1], areas[2]);

    // Inside its border, the end of a long to-do shows, with room for the
    // cursor.
    let room = input.inner(Margin::new(1, 1)).width as usize;
    let length = app.input.chars().count();
    let shown = length.min(room.saturating_sub(1));
    let text: String = app.input.chars().skip(length - shown).collect();
    let style = if app.editing {
        Style::new().fg(Color::Yellow)
    } else {
        Style::new().add_modifier(Modifier::DIM)
    };
    frame.render_widget(
        Paragraph::new(text)
            .style(style)
            .block(Block::bordered().title(" New to-do ")),
        input,
    );
    if app.editing {
        frame.set_cursor_position(Position::new(input.x + 1 + shown as u16, input.y + 1));
    }

    let left = app.todos.iter().filter(|todo| !todo.done).count();
    let rows: Vec<ListItem> = app
        .todos
        .iter()
        .map(|todo| {
            if todo.done {
                ListItem::new(format!("✓ {}", todo.text)).style(
                    Style::new()
                        .fg(Color::DarkGray)
                        .add_modifier(Modifier::CROSSED_OUT),
                )
            } else {
                ListItem::new(format!("☐ {}", todo.text))
            }
        })
        .collect();
    let mut list = ListState::default()
        .with_selected(app.selected)
        .with_offset(app.offset);
    frame.render_stateful_widget(
        List::new(rows)
            .block(Block::bordered().title(format!(" To do: {left} left ")))
            .highlight_style(Style::new().add_modifier(Modifier::REVERSED))
            .highlight_symbol("> "),
        items,
        &mut list,
    );
    // The list keeps the selection in view; the app remembers where it is.
    app.selected = list.selected();
    app.offset = list.offset();

    let keys = if app.editing {
        "Enter saves · Esc cancels"
    } else {
        "a adds · Space checks off · d deletes · ↑↓ move · q quits"
    };
    frame.render_widget(
        Paragraph::new(keys).style(Style::new().add_modifier(Modifier::DIM)),
        help,
    );
}

// rawr
//! What the keys do and what the screen says. Under `rawr run` this file is
//! interpreted, and a save shows on the screen at once. `cargo build`
//! compiles it like any other module.

use ratatui::layout::{Margin, Rect};

/// One thing to do.
pub struct Todo {
    pub text: String,
    pub done: bool,
}

/// The list, and the to-do being written.
pub struct App {
    pub todos: Vec<Todo>,
    pub input: String,
    pub editing: bool,
}

/// A key the app acts on. `main.rs` turns crossterm's keys into these.
pub enum Key {
    Char(char),
    Backspace,
    Enter,
    Esc,
}

/// One line of the list as it is drawn.
pub struct Row {
    pub text: String,
    pub done: bool,
}

/// Everything the screen says. `main.rs` draws it.
pub struct View {
    pub title: String,
    pub rows: Vec<Row>,
    pub prompt: String,
    pub input: String,
    pub cursor: u16,
    pub editing: bool,
    pub help: String,
}

impl App {
    pub fn new() -> App {
        App {
            todos: vec![
                todo("Write the view"),
                todo("Save it"),
                todo("Watch it change"),
            ],
            input: String::new(),
            editing: false,
        }
    }
}

fn todo(text: &str) -> Todo {
    Todo {
        text: text.to_string(),
        done: false,
    }
}

/// What `key` does, with `selected` the list's selected row.
pub fn update(app: &mut App, key: Key, selected: Option<usize>) {
    if app.editing {
        match key {
            Key::Char(char) => app.input.push(char),
            Key::Backspace => {
                app.input.pop();
            }
            Key::Enter => {
                let text = app.input.trim().to_string();
                if !text.is_empty() {
                    app.todos.push(Todo { text, done: false });
                }
                app.input.clear();
                app.editing = false;
            }
            Key::Esc => {
                app.input.clear();
                app.editing = false;
            }
        }
        return;
    }
    let count = app.todos.len();
    match (key, selected) {
        (Key::Char('a'), _) => app.editing = true,
        (Key::Char(' '), Some(index)) if index < count => {
            app.todos[index].done = !app.todos[index].done;
        }
        (Key::Char('d'), Some(index)) if index < count => {
            app.todos.remove(index);
        }
        _ => {}
    }
}

/// The screen for a terminal `width` columns wide.
pub fn view(app: &App, width: u16) -> View {
    let left = app.todos.iter().filter(|todo| !todo.done).count();
    let rows = app
        .todos
        .iter()
        .map(|todo| Row {
            text: todo.text.clone(),
            done: todo.done,
        })
        .collect();
    // The input box is the screen's width and three rows high. Inside its
    // border, the end of a long to-do shows, with room for the cursor.
    let input_box = Rect::new(0, 0, width, 3);
    let room = input_box.inner(Margin::new(1, 1)).width as usize;
    let length = app.input.chars().count();
    let shown = length.min(room.saturating_sub(1));
    let input: String = app.input.chars().skip(length - shown).collect();
    let help = if app.editing {
        "Enter saves · Esc cancels"
    } else {
        "a adds · Space checks off · d deletes · ↑↓ move · q quits"
    };
    View {
        title: format!(" To do: {left} left "),
        rows,
        prompt: String::from(" New to-do "),
        cursor: shown as u16,
        input,
        editing: app.editing,
        help: help.to_string(),
    }
}

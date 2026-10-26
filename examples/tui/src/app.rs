// rawr
//! What the keys do. Under `rawr run` this file is interpreted, and a save
//! takes effect at the next key. `cargo build` compiles it like any other
//! module.

/// One thing to do.
pub struct Todo {
    pub text: String,
    pub done: bool,
}

/// The list, the to-do being written, and where the list is.
pub struct App {
    pub todos: Vec<Todo>,
    pub input: String,
    pub editing: bool,
    /// The selected row, as ratatui's `ListState` keeps it.
    pub selected: Option<usize>,
    /// The first row the list shows.
    pub offset: usize,
}

/// A key the app acts on. `main.rs` turns crossterm's keys into these.
pub enum Key {
    Char(char),
    Backspace,
    Enter,
    Esc,
    Up,
    Down,
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
            selected: Some(0),
            offset: 0,
        }
    }
}

fn todo(text: &str) -> Todo {
    Todo {
        text: text.to_string(),
        done: false,
    }
}

/// What `key` does.
pub fn update(app: &mut App, key: Key) {
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
            Key::Up | Key::Down => {}
        }
        return;
    }
    let count = app.todos.len();
    match (key, app.selected) {
        (Key::Char('a'), _) => app.editing = true,
        (Key::Up, Some(index)) => app.selected = Some(index.saturating_sub(1)),
        (Key::Down, Some(index)) => app.selected = Some(index + 1),
        (Key::Up | Key::Down, None) => app.selected = Some(0),
        (Key::Char(' '), Some(index)) if index < count => {
            app.todos[index].done = !app.todos[index].done;
        }
        (Key::Char('d'), Some(index)) if index < count => {
            app.todos.remove(index);
        }
        _ => {}
    }
}

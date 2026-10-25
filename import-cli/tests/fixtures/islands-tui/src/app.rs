// rawr
//! What the terminal shows: the app's state, what a key does to it, and
//! the view native code draws.

pub struct App {
    pub items: Vec<String>,
    pub typed: String,
    pub keys: i64,
}

pub struct View {
    pub title: String,
    pub items: Vec<String>,
    pub status: String,
}

impl App {
    pub fn new() -> App {
        App {
            items: vec![
                String::from("alpha"),
                String::from("beta"),
                String::from("gamma"),
            ],
            typed: String::new(),
            keys: 0,
        }
    }
}

pub fn typed(app: &mut App, key: char) {
    app.typed.push(key);
    app.keys += 1;
}

pub fn view(app: &App) -> View {
    View {
        title: String::from("tui-one"),
        items: app.items.clone(),
        status: format!("keys={}:{}", app.keys, app.typed),
    }
}

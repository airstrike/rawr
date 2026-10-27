//! A generic entry point in the shape of a GUI library's `run`: it makes
//! the program's state with `Default`, names it by `type_name`, and feeds
//! the program's own `update` and `view` events, as an event loop would.
//! The state, the message, `update` and `view` are the script's (D594).

use std::fmt::Debug;

/// What `view` gives: a label borrowed from the state, and the message a
/// press sends.
pub struct Widget<'a, M> {
    label: &'a str,
    on_press: Option<M>,
}

/// A widget showing `text`.
pub fn label<'a, M>(text: &'a str) -> Widget<'a, M> {
    Widget {
        label: text,
        on_press: None,
    }
}

impl<'a, M> Widget<'a, M> {
    /// The message a press sends.
    pub fn on_press(mut self, message: M) -> Self {
        self.on_press = Some(message);
        self
    }
}

/// Print the state's type name and the process id, then show `view` and
/// press it `EVENTS_COUNT` times (3 by default), every 250 ms when there
/// are more than 3, until the file `EVENTS_STOP` names exists.
pub fn run<S, M, U, V>(update: U, view: V)
where
    S: Default + 'static,
    M: Clone + Send + Debug + 'static,
    U: Fn(&mut S, M),
    V: for<'a> Fn(&'a S) -> Widget<'a, M>,
{
    let stop = std::env::var_os("EVENTS_STOP");
    let events = std::env::var("EVENTS_COUNT")
        .ok()
        .and_then(|count| count.parse::<i64>().ok())
        .unwrap_or(3);
    println!("title {}", std::any::type_name::<S>());
    println!("pid {}", std::process::id());
    let mut state = S::default();
    for event in 1..=events {
        if stop
            .as_ref()
            .is_some_and(|path| std::path::Path::new(path).exists())
        {
            break;
        }
        let widget = view(&state);
        println!("{event} view {}", widget.label);
        if let Some(message) = widget.on_press.clone() {
            println!("{event} message {message:?}");
            update(&mut state, message);
        }
        if events > 3 {
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }
    println!("last view {}", view(&state).label);
}

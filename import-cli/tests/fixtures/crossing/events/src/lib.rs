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

// ---- iced's shape: `run(update, view)` through traits over `Fn` ----

/// A theme, as a GUI library's default one.
#[derive(Default)]
pub struct Theme;

/// A renderer, as a GUI library's default one.
pub struct Renderer;

/// What a library's `view` gives: anything that is an element for the
/// program's message, theme and renderer, as iced's `Widget` is.
pub trait Element<Message, Theme, Renderer> {
    /// What the widget shows.
    fn label(&self) -> &str;

    /// The message a press sends.
    fn press(&self) -> Option<Message>;
}

/// A widget with a label it borrows and a message, at a theme and renderer
/// that default to the library's own.
pub struct Column<'a, Message, Theme = crate::Theme, Renderer = crate::Renderer> {
    label: &'a str,
    on_press: Option<Message>,
    look: std::marker::PhantomData<(Theme, Renderer)>,
}

/// A column showing `text`.
pub fn column<'a, Message>(text: &'a str) -> Column<'a, Message> {
    Column {
        label: text,
        on_press: None,
        look: std::marker::PhantomData,
    }
}

impl<Message, Theme, Renderer> Column<'_, Message, Theme, Renderer> {
    /// The message a press sends.
    pub fn on_press(mut self, message: Message) -> Self {
        self.on_press = Some(message);
        self
    }
}

impl<Message: Clone, Theme, Renderer> Element<Message, Theme, Renderer>
    for Column<'_, Message, Theme, Renderer>
{
    fn label(&self) -> &str {
        self.label
    }

    fn press(&self) -> Option<Message> {
        self.on_press.clone()
    }
}

/// What `update` may give back: work to do after it, or none.
pub struct Task<Message>(Option<Message>);

impl<Message> From<()> for Task<Message> {
    fn from((): ()) -> Self {
        Task(None)
    }
}

/// A message no program sends.
pub enum Never {}

/// The update logic of a program: any `Fn(&mut State, Message)` whose
/// output turns into a task.
pub trait UpdateFn<State, Message> {
    /// Change the state for the message.
    fn update(&self, state: &mut State, message: Message) -> Task<Message>;
}

impl<State> UpdateFn<State, Never> for () {
    fn update(&self, _state: &mut State, message: Never) -> Task<Never> {
        match message {}
    }
}

impl<T, State, Message, C> UpdateFn<State, Message> for T
where
    T: Fn(&mut State, Message) -> C,
    C: Into<Task<Message>>,
{
    fn update(&self, state: &mut State, message: Message) -> Task<Message> {
        self(state, message).into()
    }
}

/// The view logic of a program: any `Fn(&State)` that gives a widget
/// borrowing the state.
pub trait ViewFn<'a, State, Message, Theme, Renderer> {
    /// The widget showing the state.
    fn view(&self, state: &'a State) -> impl Element<Message, Theme, Renderer> + 'a;
}

impl<'a, T, State, Message, Theme, Renderer, W> ViewFn<'a, State, Message, Theme, Renderer> for T
where
    T: Fn(&'a State) -> W,
    State: 'static,
    Message: 'static,
    Theme: 'static,
    Renderer: 'static,
    W: Element<Message, Theme, Renderer> + 'a,
{
    fn view(&self, state: &'a State) -> impl Element<Message, Theme, Renderer> + 'a {
        self(state)
    }
}

/// [`run`] in iced's shape: `update` and `view` through the traits above,
/// with the theme and renderer `view`'s widget names.
pub fn application<State, Message, Theme, Renderer>(
    update: impl UpdateFn<State, Message> + 'static,
    view: impl for<'a> ViewFn<'a, State, Message, Theme, Renderer> + 'static,
) where
    State: Default + 'static,
    Message: Send + Debug + Clone + 'static,
    Theme: Default + 'static,
    Renderer: 'static,
{
    let stop = std::env::var_os("EVENTS_STOP");
    let events = std::env::var("EVENTS_COUNT")
        .ok()
        .and_then(|count| count.parse::<i64>().ok())
        .unwrap_or(3);
    println!("title {}", std::any::type_name::<State>());
    println!("theme {}", std::any::type_name::<Theme>());
    println!("pid {}", std::process::id());
    let _theme = Theme::default();
    let mut state = State::default();
    for event in 1..=events {
        if stop
            .as_ref()
            .is_some_and(|path| std::path::Path::new(path).exists())
        {
            break;
        }
        let pressed = {
            let widget = view.view(&state);
            println!("{event} view {}", widget.label());
            widget.press()
        };
        if let Some(message) = pressed {
            println!("{event} message {message:?}");
            let _task = update.update(&mut state, message);
        }
        if events > 3 {
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
    }
    println!("last view {}", view.view(&state).label());
}

//! A generic entry point in the shape of a GUI library's `run`: it makes
//! the program's state with `Default`, names it by `type_name`, and feeds
//! the program's own `update` and `view` events, as an event loop would.
//! The state, the message, `update` and `view` are the script's (D594).

use std::fmt::Debug;

pub mod controls;
pub mod conversions;

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

/// A theme, as a GUI library's default one, which clones as iced's does.
#[derive(Default, Clone)]
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

impl<T> Task<T> {
    /// No work.
    pub fn none() -> Self {
        Task(None)
    }

    /// The work that gives `value`.
    pub fn done(value: T) -> Self {
        Task(Some(value))
    }
}

/// The work of finding the latest window's id, as iced's
/// `window::latest()`: there is none when no window is open.
pub fn latest() -> Task<Option<u32>> {
    Task(Some(Some(7)))
}

/// The work of closing the window `id`, as iced's `window::close`, which
/// gives no message.
pub fn close<T>(id: u32) -> Task<T> {
    println!("close {id}");
    Task(None)
}

impl<T> Task<Option<T>> {
    /// The work `f` gives for the value, when this work gives `Some`, as
    /// iced's `and_then` of a `Task<Option<T>>`.
    pub fn and_then<A>(self, f: impl Fn(T) -> Task<A> + 'static) -> Task<A> {
        match self.0 {
            Some(Some(value)) => f(value),
            _ => Task(None),
        }
    }
}

impl<T, E> Task<Result<T, E>> {
    /// The work `f` gives for the value, when this work succeeds, as
    /// iced's `and_then` of a `Task<Result<T, E>>`.
    pub fn and_then<A>(self, f: impl Fn(T) -> Task<Result<A, E>> + 'static) -> Task<Result<A, E>> {
        match self.0 {
            Some(Ok(value)) => f(value),
            Some(Err(error)) => Task(Some(Err(error))),
            None => Task(None),
        }
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

// ---- iced's counter: a view only `impl Widget<Message>`'s defaults fix ----

/// A view's widgets in the shape of iced's: the renderer is named only by
/// a parameterless alias of a generic type at a backend no path reaches,
/// `Widget`'s defaulted `Renderer` is that alias, and nothing a view calls
/// names a renderer: the crate's impls and the trait's defaults decide it.
pub mod widgets {
    use std::fmt::{Debug, Display};

    mod backend {
        /// A backend no public path reaches.
        pub struct Gpu;
        /// Another.
        pub struct Soft;
    }

    pub mod fallback {
        /// A renderer over two backends.
        pub struct Renderer<A, B>(std::marker::PhantomData<(A, B)>);

        /// What a renderer over two backends draws.
        pub enum Geometry<A, B> {
            /// Drawn by the first backend.
            Primary(String, std::marker::PhantomData<A>),
            /// Drawn by the second.
            Secondary(String, std::marker::PhantomData<B>),
        }
    }

    /// The only name of the renderer the crate draws with.
    pub type Renderer = fallback::Renderer<backend::Gpu, backend::Soft>;

    /// What a view gives, at a theme and renderer that default to the
    /// crate's.
    pub trait Widget<Message, Theme = crate::Theme, Renderer = crate::widgets::Renderer> {
        /// The labels it shows, in order.
        fn labels(&self, labels: &mut Vec<String>);

        /// The message a press on the button labelled `label` sends.
        fn press(&self, label: &str) -> Option<Message>;

        /// The widget as an element.
        fn boxed<'a>(self) -> element::Element<'a, Message, Theme, Renderer>
        where
            Self: Sized + 'a,
        {
            element::Element(Box::new(self))
        }
    }

    /// Methods every widget has through one blanket impl over the crate's
    /// own `Widget`, as iced's `Widget` extends `core::Widget`. No widget
    /// implements it itself.
    pub trait Framed<Message, Theme = crate::Theme, Renderer = crate::widgets::Renderer>:
        Widget<Message, Theme, Renderer>
    {
        /// The widget as an element, as iced's `Widget::boxed`.
        fn framed<'a>(self) -> element::Element<'a, Message, Theme, Renderer>
        where
            Self: Sized + 'a,
        {
            Widget::boxed(self)
        }
    }

    impl<T, Message, Theme, Renderer> Framed<Message, Theme, Renderer> for T where
        T: Widget<Message, Theme, Renderer>
    {
    }

    pub mod element {
        /// Any widget, boxed.
        pub struct Element<'a, Message, Theme, Renderer>(
            pub(crate) Box<dyn super::Widget<Message, Theme, Renderer> + 'a>,
        );
    }

    /// An element at the crate's theme and renderer.
    pub type Element<'a, Message, Theme = crate::Theme, Renderer = crate::widgets::Renderer> =
        element::Element<'a, Message, Theme, Renderer>;

    impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
        for element::Element<'_, Message, Theme, Renderer>
    {
        fn labels(&self, labels: &mut Vec<String>) {
            self.0.labels(labels);
        }

        fn press(&self, label: &str) -> Option<Message> {
            self.0.press(label)
        }
    }

    /// A button showing `content`.
    pub struct Button<'a, Message, W, Theme = crate::Theme> {
        content: W,
        on_press: Option<Message>,
        look: std::marker::PhantomData<&'a Theme>,
    }

    /// A button showing `content`, which a string literal fills as
    /// `&'static str`.
    pub fn button<'a, Message, W, Theme>(content: W) -> Button<'a, Message, W, Theme> {
        Button {
            content,
            on_press: None,
            look: std::marker::PhantomData,
        }
    }

    impl<Message, W, Theme> Button<'_, Message, W, Theme> {
        /// The message a press sends.
        pub fn on_press(mut self, message: Message) -> Self {
            self.on_press = Some(message);
            self
        }
    }

    impl<Message: Clone, W: Display, Theme, Renderer> Widget<Message, Theme, Renderer>
        for Button<'_, Message, W, Theme>
    {
        fn labels(&self, labels: &mut Vec<String>) {
            labels.push(format!("[{}]", self.content));
        }

        fn press(&self, label: &str) -> Option<Message> {
            (self.content.to_string() == label)
                .then(|| self.on_press.clone())
                .flatten()
        }
    }

    /// Text showing a value, at a size.
    pub struct Text<'a, Theme = crate::Theme> {
        text: String,
        size: u32,
        look: std::marker::PhantomData<&'a Theme>,
    }

    /// Text showing `value`.
    pub fn text<'a, Theme>(value: impl ToString) -> Text<'a, Theme> {
        Text {
            text: value.to_string(),
            size: 16,
            look: std::marker::PhantomData,
        }
    }

    impl<Theme> Text<'_, Theme> {
        /// The text at `size`.
        pub fn size(mut self, size: u32) -> Self {
            self.size = size;
            self
        }
    }

    impl<'a, Theme> Text<'a, Theme> {
        /// Text showing `value`, as iced's `Text::new`.
        pub fn new(value: impl ToString) -> Self {
            text(value)
        }
    }

    /// Text at the crate's theme: an alias that passes its parameters on
    /// to the type, as iced's `widget::Text` is `core::widget::Text`.
    pub type Label<'a, Theme = crate::Theme> = Text<'a, Theme>;

    /// Text of a format string, as iced's `text!`: the type's associated
    /// function reached through the alias. It shares `text`'s name.
    #[macro_export]
    macro_rules! widgets_text {
        ($($arg:tt)*) => {
            $crate::widgets::Label::new(format!($($arg)*))
        };
    }

    pub use crate::widgets_text as text;

    impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Text<'_, Theme> {
        fn labels(&self, labels: &mut Vec<String>) {
            labels.push(format!("{}@{}", self.text, self.size));
        }

        fn press(&self, _label: &str) -> Option<Message> {
            None
        }
    }

    /// Widgets one above another.
    pub struct Column<W> {
        children: Vec<W>,
        padding: u16,
    }

    impl<W> Column<W> {
        /// A column of `children`.
        pub fn with_children(children: impl IntoIterator<Item = W>) -> Self {
            Column {
                children: children.into_iter().collect(),
                padding: 0,
            }
        }

        /// The column with `padding` around it.
        pub fn padding(mut self, padding: u16) -> Self {
            self.padding = padding;
            self
        }
    }

    impl<W, Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Column<W>
    where
        W: Widget<Message, Theme, Renderer>,
    {
        fn labels(&self, labels: &mut Vec<String>) {
            labels.push(format!("pad {}", self.padding));
            for child in &self.children {
                child.labels(labels);
            }
        }

        fn press(&self, label: &str) -> Option<Message> {
            self.children.iter().find_map(|child| child.press(label))
        }
    }

    /// A column of widgets, each boxed, as iced's `column!`. Its own name
    /// differs from the root's `column` function's.
    #[macro_export]
    macro_rules! widgets_column {
        ($($x:expr),+ $(,)?) => (
            $crate::widgets::Column::with_children([$($crate::widgets::Widget::boxed($x)),+])
        );
    }

    pub use crate::widgets_column as column;

    /// The view logic of a program: any `Fn(&State)` that gives a widget.
    pub trait ViewFn<'a, State, Message, Theme, Renderer> {
        /// The widget showing the state.
        fn view(&self, state: &'a State) -> impl Widget<Message, Theme, Renderer> + 'a;
    }

    impl<'a, T, State, Message, Theme, Renderer, W> ViewFn<'a, State, Message, Theme, Renderer> for T
    where
        T: Fn(&'a State) -> W,
        State: 'static,
        Message: 'static,
        Theme: 'static,
        Renderer: 'static,
        W: Widget<Message, Theme, Renderer> + 'a,
    {
        fn view(&self, state: &'a State) -> impl Widget<Message, Theme, Renderer> + 'a {
            self(state)
        }
    }

    /// Show `view` and press `Increment` `EVENTS_COUNT` times (3 by
    /// default), then `Decrement` once, every 250 ms when there are more
    /// than 3, until the file `EVENTS_STOP` names exists.
    pub fn run<State, Message, Theme, Renderer>(
        update: impl crate::UpdateFn<State, Message> + 'static,
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
        println!("renderer {}", std::any::type_name::<Renderer>());
        println!("pid {}", std::process::id());
        let mut state = State::default();
        for event in 1..=events + 1 {
            if stop
                .as_ref()
                .is_some_and(|path| std::path::Path::new(path).exists())
            {
                break;
            }
            let label = if event > events {
                "Decrement"
            } else {
                "Increment"
            };
            let pressed = {
                let widget = view.view(&state);
                let mut labels = Vec::new();
                widget.labels(&mut labels);
                println!("{event} view {}", labels.join(" "));
                widget.press(label)
            };
            if let Some(message) = pressed {
                println!("{event} message {message:?}");
                let _task = update.update(&mut state, message);
            }
            if events > 3 {
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
        }
        let widget = view.view(&state);
        let mut labels = Vec::new();
        widget.labels(&mut labels);
        println!("last view {}", labels.join(" "));
    }
}

// ---- iced's canvas: aliases of types only the aliases name ----

/// iced's `widget::canvas` aliases. Their targets are in `parts`, which
/// this crate depends on and re-exports none of, as iced names iced_core's
/// and iced_graphics's types only through its aliases.
pub mod canvas {
    use crate::widgets::fallback;

    /// A cache at the crate's renderer: an alias that passes its parameter
    /// on, defaulted to another alias, as iced's `canvas::Cache<Renderer =
    /// crate::Renderer> = geometry::Cache<Renderer>`.
    pub type Cache<Renderer = crate::widgets::Renderer> = parts::geometry::Cache<Renderer>;

    /// What a renderer draws: an alias of an associated type, as iced's
    /// `canvas::Geometry<Renderer = crate::Renderer> = <Renderer as
    /// geometry::Renderer>::Geometry`.
    pub type Geometry<Renderer = crate::widgets::Renderer> =
        <Renderer as parts::geometry::Renderer>::Geometry;

    /// Text at the crate's theme: an alias that passes a lifetime and a
    /// defaulted theme on, as iced's `widget::Text`.
    pub type Note<'a, Theme = crate::Theme> = parts::Note<'a, Theme>;

    impl<A, B> parts::geometry::Renderer for fallback::Renderer<A, B> {
        type Geometry = fallback::Geometry<A, B>;

        fn draw(label: &str) -> Self::Geometry {
            fallback::Geometry::Primary(label.to_owned(), std::marker::PhantomData)
        }
    }

    /// `label`, drawn by the crate's renderer.
    pub fn sketch(label: &str) -> Geometry {
        <crate::widgets::Renderer as parts::geometry::Renderer>::draw(label)
    }

    /// What `geometry` shows.
    pub fn describe(geometry: &Geometry) -> String {
        match geometry {
            fallback::Geometry::Primary(label, _) => format!("primary {label}"),
            fallback::Geometry::Secondary(label, _) => format!("secondary {label}"),
        }
    }

    // ---- iced's canvas::Program: a trait a program's own type implements ----

    /// Where a program draws, as iced's `Rectangle`.
    #[derive(Debug, Clone, Copy, PartialEq)]
    pub struct Bounds {
        /// Its width.
        pub width: u32,
        /// Its height.
        pub height: u32,
    }

    impl Bounds {
        /// Its area.
        pub fn area(&self) -> u32 {
            self.width * self.height
        }
    }

    /// What a canvas gives its program, as iced's canvas `Event`.
    #[derive(Debug, Clone, PartialEq)]
    pub enum Event {
        /// The button labelled with the string was pressed.
        Pressed(String),
    }

    /// The pen a renderer draws with, as iced's renderer is lent to `draw`.
    pub struct Pen {
        strokes: std::cell::Cell<u32>,
    }

    impl Pen {
        /// `label`, drawn with this pen.
        pub fn stroke(&self, label: &str) -> Geometry {
            self.strokes.set(self.strokes.get() + 1);
            <crate::widgets::Renderer as parts::geometry::Renderer>::draw(&format!(
                "{label}#{}",
                self.strokes.get()
            ))
        }
    }

    /// What a canvas draws and how it answers events, as iced's
    /// `canvas::Program`: a program's own type implements it, and the
    /// canvas calls it back while it lays out and while it handles a press.
    pub trait Program<Message, Theme = crate::Theme> {
        /// What the canvas keeps for the program between calls.
        type State: Default + 'static;

        /// What a press gives, if anything.
        fn update(
            &self,
            _state: &mut Self::State,
            _event: &Event,
            _bounds: Bounds,
        ) -> Option<Message> {
            None
        }

        /// What the program draws at `bounds` with `pen`.
        fn draw(
            &self,
            state: &Self::State,
            pen: &Pen,
            theme: &Theme,
            bounds: Bounds,
        ) -> Vec<Geometry>;
    }

    impl<Message, Theme, T> Program<Message, Theme> for &T
    where
        T: Program<Message, Theme>,
    {
        type State = T::State;

        fn update(
            &self,
            state: &mut Self::State,
            event: &Event,
            bounds: Bounds,
        ) -> Option<Message> {
            T::update(self, state, event, bounds)
        }

        fn draw(
            &self,
            state: &Self::State,
            pen: &Pen,
            theme: &Theme,
            bounds: Bounds,
        ) -> Vec<Geometry> {
            T::draw(self, state, pen, theme, bounds)
        }
    }

    /// A widget a program draws, as iced's `Canvas`.
    pub struct Canvas<P, Message, Theme = crate::Theme>
    where
        P: Program<Message, Theme>,
    {
        program: P,
        state: std::cell::RefCell<P::State>,
        look: std::marker::PhantomData<(Message, Theme)>,
    }

    /// A canvas `program` draws, as iced's `canvas(..)`.
    pub fn canvas<P, Message, Theme>(program: P) -> Canvas<P, Message, Theme>
    where
        P: Program<Message, Theme>,
    {
        Canvas {
            program,
            state: std::cell::RefCell::new(P::State::default()),
            look: std::marker::PhantomData,
        }
    }

    const BOUNDS: Bounds = Bounds {
        width: 40,
        height: 30,
    };

    impl<P, Message, Theme> crate::widgets::Widget<Message, Theme> for Canvas<P, Message, Theme>
    where
        P: Program<Message, Theme>,
        Theme: Default,
    {
        fn labels(&self, labels: &mut Vec<String>) {
            labels.push(self.drawn());
        }

        /// The program's `update`, then what it draws with the state the
        /// update left.
        fn press(&self, label: &str) -> Option<Message> {
            let message = self.program.update(
                &mut self.state.borrow_mut(),
                &Event::Pressed(label.to_owned()),
                BOUNDS,
            );
            println!("pressed {label}: {}", self.drawn());
            message
        }
    }

    impl<P, Message, Theme> Canvas<P, Message, Theme>
    where
        P: Program<Message, Theme>,
        Theme: Default,
    {
        fn drawn(&self) -> String {
            let pen = Pen {
                strokes: std::cell::Cell::new(0),
            };
            let drawn = self
                .program
                .draw(&self.state.borrow(), &pen, &Theme::default(), BOUNDS);
            format!(
                "canvas[{}]",
                drawn.iter().map(describe).collect::<Vec<_>>().join(", ")
            )
        }
    }
}

impl Theme {
    /// The theme's name.
    pub fn name(&self) -> String {
        "plain".to_owned()
    }
}

/// A color a crate draws with, as iced's `Color`, which a program's own
/// colors convert into.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    /// Its red.
    pub r: u8,
    /// Its green.
    pub g: u8,
    /// Its blue.
    pub b: u8,
}

impl Color {
    /// Black, as iced's `Color::BLACK`.
    pub const BLACK: Color = Color { r: 0, g: 0, b: 0 };

    /// The color of `r`, `g` and `b`, as iced's `Color::from_rgb8`.
    pub fn rgb(r: u8, g: u8, b: u8) -> Self {
        Color { r, g, b }
    }
}

/// `color` as a crate takes a color: anything that converts into one, as
/// iced's `text(..).color(..)` takes `impl Into<Color>`.
pub fn paint(color: impl Into<Color>) -> String {
    let color = color.into();
    format!("rgb({}, {}, {})", color.r, color.g, color.b)
}

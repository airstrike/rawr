//! A generic entry point in the shape of a GUI library's `run`: it makes
//! the program's state with `Default`, names it by `type_name`, and feeds
//! the program's own `update` and `view` events, as an event loop would.
//! The state, the message, `update` and `view` are the script's (D594).

use std::fmt::Debug;

pub mod buffers;
pub mod choices;
pub mod classic;
pub mod color;
pub mod controls;
pub mod conversions;
pub mod custom;
pub mod fonts;
pub mod grid;
pub mod keyed;
pub mod matched;
pub mod named;
pub mod ordered;
pub mod plane;
pub mod pointers;
pub mod results;
pub mod selector;
pub mod sequences;
pub mod signatures;

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

/// What `update` may give back: work to do after it, or none. Its
/// parameter is named as its impls name theirs, as iced's `Task<T>` is.
pub struct Task<T>(Option<T>);

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

    /// The work that gives `value`, as iced's, which bounds its value.
    pub fn done(value: T) -> Self
    where
        T: MaybeSend + 'static,
    {
        Task(Some(value))
    }

    /// The value this work gives, when it gives one.
    pub fn value(self) -> Option<T> {
        self.0
    }

    /// iced's `Task::future`: the work that gives `future`'s output. It is
    /// polled here until it ends, as an executor would.
    pub fn future(future: impl std::future::Future<Output = T> + MaybeSend + 'static) -> Self
    where
        T: MaybeSend + 'static,
    {
        let mut future = Box::pin(future);
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        for _ in 0..100_000 {
            if let std::task::Poll::Ready(output) = future.as_mut().poll(&mut context) {
                return Task(Some(output));
            }
            std::thread::yield_now();
        }
        panic!("the future never ended");
    }

    /// iced's `Task::then`: the work `f` makes of this work's value.
    pub fn then<O>(self, mut f: impl FnMut(T) -> Task<O> + MaybeSend + 'static) -> Task<O>
    where
        T: MaybeSend + 'static,
        O: MaybeSend + 'static,
    {
        Task(self.0.and_then(|value| f(value).0))
    }

    /// The work that gives `f` of this work's value, which `f` makes on
    /// another thread, as iced's executor runs `Task::map`'s function.
    pub fn map<A>(self, mut f: impl FnMut(T) -> A + MaybeSend + 'static) -> Task<A>
    where
        T: MaybeSend + 'static,
        A: MaybeSend + 'static,
    {
        Task(
            std::thread::spawn(move || self.0.map(&mut f))
                .join()
                .expect("the mapping thread finishes"),
        )
    }
}

/// Work that reports its steps, as sipper's `Sipper` reports progress. A
/// program's own native code gives it behind `impl Steps`.
pub trait Steps {
    /// Each step, in order.
    fn steps(self) -> Vec<u32>;
}

impl<I: Iterator<Item = u32>> Steps for I {
    fn steps(self) -> Vec<u32> {
        self.collect()
    }
}

/// Work that reports items of its own type, as sipper's `Sipper` names
/// its progress `Stream::Item`.
pub trait Sip {
    /// What it reports.
    type Item;

    /// Each item it reports, in order.
    fn sipped(self) -> Vec<Self::Item>;
}

impl<T> Task<T> {
    /// The work that gives `f` of the last of `steps`, as iced's
    /// `Task::sip` gives its output's message.
    pub fn stepped<S>(steps: S, f: impl FnOnce(u32) -> T + MaybeSend + 'static) -> Self
    where
        S: Steps + MaybeSend + 'static,
        T: MaybeSend + 'static,
    {
        Task(steps.steps().pop().map(f))
    }

    /// iced's `Task::sip`: `f` takes the sipper's own `Item`.
    pub fn sip<S>(sipper: S, f: impl FnMut(S::Item) -> T + MaybeSend + 'static) -> Self
    where
        S: Sip + MaybeSend + 'static,
        T: MaybeSend + 'static,
    {
        Task(sipper.sipped().into_iter().map(f).last())
    }

    /// iced's `Task::sip` whole: `on_progress` takes the sipper's `Item`
    /// and `on_output` its `<S as Future>::Output`, both reached through
    /// `Core`'s supertraits. The task's value is the output's message.
    pub fn finish<S>(
        mut sipper: S,
        on_progress: impl FnMut(S::Item) -> T + MaybeSend + 'static,
        on_output: impl FnOnce(<S as std::future::Future>::Output) -> T + MaybeSend + 'static,
    ) -> Self
    where
        S: Core + MaybeSend + 'static,
        T: MaybeSend + 'static,
    {
        let mut context = std::task::Context::from_waker(std::task::Waker::noop());
        let output = match std::pin::Pin::new(&mut sipper).poll(&mut context) {
            std::task::Poll::Ready(output) => output,
            std::task::Poll::Pending => return Task(None),
        };
        let progress = sipper.sipped().into_iter().map(on_progress);
        Task(progress.chain([on_output(output)]).last())
    }
}

/// Work that is a future and reports items, as sipper's `Core` is a
/// `Future` and a `Stream`.
pub trait Core: std::future::Future + Sip + Unpin {}

impl<T: std::future::Future + Sip + Unpin> Core for T {}

/// As sipper's `Sipper<Output, Progress>: Core<Output = Output, Item =
/// Progress>`.
pub trait Sipper<Output, Progress = Output>: Core<Output = Output, Item = Progress> {}

impl<T, Output, Progress> Sipper<Output, Progress> for T where
    T: Core<Output = Output, Item = Progress>
{
}

/// As sipper's `Straw<Output, Progress, Error>`: a sipper that can fail.
pub trait Straw<Output, Progress = Output, Error = ()>:
    Sipper<Result<Output, Error>, Progress>
{
}

impl<S, Output, Progress, Error> Straw<Output, Progress, Error> for S where
    S: Sipper<Result<Output, Error>, Progress>
{
}

/// What may cross to another thread, as iced's `MaybeSend` on native.
pub trait MaybeSend: Send {}

impl<T: Send> MaybeSend for T {}

/// A function of two inputs with its first one given, as iced's
/// `Function`: every `Fn(A, B) -> O` has it through the blanket impl.
pub trait Function<A, B, O> {
    /// The function that calls this one with `prefix` first.
    fn with(self, prefix: A) -> impl Fn(B) -> O;
}

impl<F, A, B, O> Function<A, B, O> for F
where
    F: Fn(A, B) -> O,
    Self: Sized,
    A: Clone,
{
    fn with(self, prefix: A) -> impl Fn(B) -> O {
        move |result| self(prefix.clone(), result)
    }
}

/// A font family, as iced's `font::Family`: a crate enum that `Display`s.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Family {
    /// Serif.
    Serif,
    /// Sans serif.
    SansSerif,
    /// Monospace.
    Monospace,
}

impl std::fmt::Display for Family {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Family::Serif => "Serif",
            Family::SansSerif => "Sans",
            Family::Monospace => "Mono",
        })
    }
}

/// A graphics backend, as iced's `Backend`: a crate enum that isn't
/// `Copy`, listed by a constant slice and `Display`ed.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Backend {
    /// The best one available.
    #[default]
    Best,
    /// Drawn in software.
    Software,
    /// A backend the program names.
    Custom(String),
}

impl Backend {
    /// The backends a program may choose.
    pub const ALL: &'static [Self] = &[Self::Best, Self::Software];
}

impl std::fmt::Display for Backend {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Backend::Best => f.write_str("Best"),
            Backend::Software => f.write_str("Software"),
            Backend::Custom(name) => f.write_str(name),
        }
    }
}

/// Why the families weren't listed, as iced's empty `font::Error`.
#[derive(Debug, Clone)]
pub enum Error {}

/// The work of listing the system's font families, as iced's
/// `font::list()`.
pub fn families() -> Task<Result<Vec<Family>, Error>> {
    Task(Some(Ok(vec![Family::Monospace, Family::Serif])))
}

/// What iced's `system::information()` gives: a crate value of fields.
#[derive(Debug, Clone)]
pub struct Info {
    /// The system's name.
    pub name: String,
    /// Its processor cores, when known.
    pub cores: Option<u32>,
}

/// The work of reading the system's information, as iced's
/// `system::information()`.
pub fn information() -> Task<Info> {
    Task(Some(Info {
        name: "fixture".to_string(),
        cores: Some(4),
    }))
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

/// A message no program sends: std's `Infallible`, as iced's `Never` is.
pub use std::convert::Infallible as Never;

/// iced's `advanced`: iced_graphics's modules, which another crate defines.
pub mod advanced {
    pub use parts::renderer::{self, Renderer};

    pub mod graphics {
        pub use parts::graphics::{color, mesh};
    }
}

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

        /// The widget with each message it sends turned by `f`, as iced's
        /// `Widget::map`, whose output names the callable's type. `Map` is
        /// `parts`'s, which this crate doesn't re-export, as iced's
        /// `Widget::map` gives iced_core's `widget::Map`.
        fn map<F, B>(self, f: F) -> parts::widget::Map<Self, F, Message>
        where
            Self: Sized,
            F: Fn(Message) -> B,
        {
            parts::widget::Map::new(self, f)
        }
    }

    impl<W, A, B, F, Theme, Renderer> Widget<B, Theme, Renderer> for parts::widget::Map<W, F, A>
    where
        W: Widget<A, Theme, Renderer>,
        F: Fn(A) -> B,
    {
        fn labels(&self, labels: &mut Vec<String>) {
            labels.push("map".to_owned());
            self.widget().labels(labels);
        }

        fn press(&self, label: &str) -> Option<B> {
            self.widget().press(label).map(self.mapper())
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

    /// A button whose content is itself a widget, as iced's `Button<'a,
    /// Message, W, Theme>` is: its impl of `Widget` holds only when its
    /// content's does, and a string literal's is the impl for `&str` in
    /// `controls`.
    pub struct Tile<'a, Message, W, Theme = crate::Theme> {
        content: W,
        on_press: Option<Message>,
        look: std::marker::PhantomData<&'a Theme>,
    }

    /// A tile showing `content`.
    pub fn tile<'a, Message, W, Theme>(content: W) -> Tile<'a, Message, W, Theme> {
        Tile {
            content,
            on_press: None,
            look: std::marker::PhantomData,
        }
    }

    impl<Message, W, Theme> Tile<'_, Message, W, Theme> {
        /// The message a press sends.
        pub fn on_press(mut self, message: Message) -> Self {
            self.on_press = Some(message);
            self
        }
    }

    impl<Message: Clone, W, Theme, Renderer> Widget<Message, Theme, Renderer>
        for Tile<'_, Message, W, Theme>
    where
        W: Widget<Message, Theme, Renderer>,
    {
        fn labels(&self, labels: &mut Vec<String>) {
            let mut content = Vec::new();
            self.content.labels(&mut content);
            labels.push(format!("<{}>", content.join(" ")));
        }

        fn press(&self, label: &str) -> Option<Message> {
            let mut content = Vec::new();
            self.content.labels(&mut content);
            (content.join(" ").trim_matches('\'') == label)
                .then(|| self.on_press.clone())
                .flatten()
        }
    }

    /// Text showing a value, at a size. It is invariant in `'a`, as iced's
    /// `Text<'a>` is through its `Theme::Class<'a>`.
    pub struct Text<'a, Theme = crate::Theme> {
        text: String,
        size: u32,
        look: std::marker::PhantomData<fn(&'a Theme) -> &'a Theme>,
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

    /// An optional widget is a widget that shows nothing when it is
    /// `None`, as iced's `impl Widget for Option<T>` makes
    /// `column![.., maybe.map(|x| text(x))]` box an `Option`.
    impl<T, Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Option<T>
    where
        T: Widget<Message, Theme, Renderer>,
    {
        fn labels(&self, labels: &mut Vec<String>) {
            match self {
                Some(widget) => widget.labels(labels),
                None => labels.push("none".to_string()),
            }
        }

        fn press(&self, label: &str) -> Option<Message> {
            self.as_ref().and_then(|widget| widget.press(label))
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

    /// `content` behind an opaque widget of its own, as iced's `opaque`,
    /// whose output names the theme and renderer only as parameters.
    pub fn opaque<'a, Message, Theme, Renderer>(
        content: impl Widget<Message, Theme, Renderer> + 'a,
    ) -> impl Widget<Message, Theme, Renderer> + 'a
    where
        Message: 'a,
        Theme: 'a,
        Renderer: 'a,
    {
        struct Opaque<W>(W);

        impl<W, Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Opaque<W>
        where
            W: Widget<Message, Theme, Renderer>,
        {
            fn labels(&self, labels: &mut Vec<String>) {
                labels.push("opaque".to_owned());
                self.0.labels(labels);
            }

            fn press(&self, label: &str) -> Option<Message> {
                self.0.press(label)
            }
        }

        Opaque(content)
    }

    /// A column of one widget, as iced's `center` is a container of one.
    pub fn center<W>(content: W) -> Column<W> {
        Column {
            children: vec![content],
            padding: 0,
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

    /// A traced shape, re-exported as iced's `canvas::{Path, path}` are.
    pub use parts::geometry::{path, Frame, Path};

    /// How a path is outlined, re-exported as iced's `canvas::{LineDash,
    /// Stroke, stroke}` are.
    pub use parts::geometry::{stroke, LineDash, Stroke};

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

    /// What a program's update gives, as iced's `canvas::Action`.
    pub struct Action<Message> {
        message: Option<Message>,
        captured: bool,
    }

    impl<Message> Action<Message> {
        /// An action that publishes `message`.
        pub fn publish(message: Message) -> Self {
            Self {
                message: Some(message),
                captured: false,
            }
        }

        /// The same action, which also captures the event.
        pub fn and_capture(mut self) -> Self {
            self.captured = true;
            self
        }

        /// What it publishes and whether it captures.
        pub fn summary(&self) -> String
        where
            Message: std::fmt::Debug,
        {
            format!("{:?} captured {}", self.message, self.captured)
        }
    }

    /// An angle, as iced's `Degrees`: it compares with a number through
    /// impls that write only `eq` and `partial_cmp`, so `<` and its kin
    /// are std's provided methods of `PartialOrd<f32>`.
    #[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
    pub struct Degrees(pub f32);

    impl PartialEq<f32> for Degrees {
        fn eq(&self, other: &f32) -> bool {
            self.0.eq(other)
        }
    }

    impl PartialOrd<f32> for Degrees {
        fn partial_cmp(&self, other: &f32) -> Option<std::cmp::Ordering> {
            self.0.partial_cmp(other)
        }
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

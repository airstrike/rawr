//! Widgets in the shape of iced's checkbox, slider and tooltip examples:
//! an `Option` of a callable, a crate function where a style closure goes,
//! a widget invariant in the lifetime of the label it borrows, an opaque
//! `impl Widget` a function gives, an application an opaque `impl Program`
//! carries, a field of a type whose `Default` the crate derives, and, in
//! the shape of iced's pick_list, a choice of script values and a string
//! literal as a widget, and, in the shape of iced's events and
//! url_handler, an application's subscription to the messages the state
//! listens for.

use std::borrow::Cow;
use std::fmt::Debug;

use crate::widgets::Widget;

/// What a style function gives.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Style {
    /// Its tone.
    pub tone: u8,
}

/// A control's status.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    /// It takes presses.
    Active,
    /// It takes none.
    Disabled,
}

/// The primary style, by the theme and the status.
pub fn primary(_theme: &crate::Theme, status: Status) -> Style {
    Style {
        tone: match status {
            Status::Active => 1,
            Status::Disabled => 2,
        },
    }
}

/// A rounded box, by the theme.
pub fn rounded_box(_theme: &crate::Theme) -> Style {
    Style { tone: 9 }
}

/// A toggle that borrows its label and boxes its callables, so it is
/// invariant in their lifetime, as iced's `Checkbox` is.
pub struct Toggle<'a, Message, Theme = crate::Theme> {
    checked: bool,
    label: &'a str,
    on_toggle: Option<Box<dyn Fn(bool) -> Message + 'a>>,
    style: Box<dyn Fn(&Theme, Status) -> Style + 'a>,
}

/// A toggle, checked or not.
pub fn toggle<'a, Message, Theme>(checked: bool) -> Toggle<'a, Message, Theme> {
    Toggle {
        checked,
        label: "",
        on_toggle: None,
        style: Box::new(|_, _| Style { tone: 0 }),
    }
}

/// What a label borrows, as iced's `IntoFragment<'a>` is.
pub trait IntoLabel<'a> {
    /// The text.
    fn into_label(self) -> &'a str;
}

impl<'a> IntoLabel<'a> for &'a str {
    fn into_label(self) -> &'a str {
        self
    }
}

impl<'a> IntoLabel<'a> for &'a String {
    fn into_label(self) -> &'a str {
        self
    }
}

impl<'a, Message, Theme> Toggle<'a, Message, Theme> {
    /// The toggle with `label`, which it borrows.
    pub fn label(mut self, label: impl IntoLabel<'a>) -> Self {
        self.label = label.into_label();
        self
    }

    /// The message a press sends, by the state it toggles to.
    pub fn on_toggle<F>(mut self, f: F) -> Self
    where
        F: Fn(bool) -> Message + 'a,
    {
        self.on_toggle = Some(Box::new(f));
        self
    }

    /// The message a press sends, if any.
    pub fn on_toggle_maybe<F>(mut self, f: Option<F>) -> Self
    where
        F: Fn(bool) -> Message + 'a,
    {
        self.on_toggle = f.map(|f| Box::new(f) as Box<dyn Fn(bool) -> Message + 'a>);
        self
    }

    /// The toggle's style.
    pub fn style(mut self, style: impl Fn(&Theme, Status) -> Style + 'a) -> Self {
        self.style = Box::new(style);
        self
    }
}

impl<Message, Theme: Default, Renderer> Widget<Message, Theme, Renderer>
    for Toggle<'_, Message, Theme>
{
    fn labels(&self, labels: &mut Vec<String>) {
        let status = if self.on_toggle.is_some() {
            Status::Active
        } else {
            Status::Disabled
        };
        labels.push(format!(
            "<{}{}:{}>",
            if self.checked { "x " } else { "" },
            self.label,
            (self.style)(&Theme::default(), status).tone
        ));
    }

    fn press(&self, label: &str) -> Option<Message> {
        (label == self.label)
            .then(|| self.on_toggle.as_ref().map(|f| f(!self.checked)))
            .flatten()
    }
}

/// A box around a hint, at a style, as iced's `tooltip(.., "hint", ..)`
/// takes the hint as a string.
pub struct Boxed<'a, Theme = crate::Theme> {
    hint: String,
    style: Box<dyn Fn(&Theme) -> Style + 'a>,
}

/// `hint` in a box.
pub fn boxed<'a, Theme>(hint: impl ToString) -> Boxed<'a, Theme> {
    Boxed {
        hint: hint.to_string(),
        style: Box::new(|_| Style { tone: 0 }),
    }
}

impl<'a, Theme> Boxed<'a, Theme> {
    /// The box's style.
    pub fn style(mut self, style: impl Fn(&Theme) -> Style + 'a) -> Self {
        self.style = Box::new(style);
        self
    }
}

impl<Message, Theme: Default, Renderer> Widget<Message, Theme, Renderer> for Boxed<'_, Theme> {
    fn labels(&self, labels: &mut Vec<String>) {
        labels.push(format!(
            "[{}:{}]",
            self.hint,
            (self.style)(&Theme::default()).tone
        ));
    }

    fn press(&self, _label: &str) -> Option<Message> {
        None
    }
}

/// A widget whose type no Rust code can write, as iced's `iced(..)` gives.
pub fn logo<'a, Message, Theme, Renderer>(
    size: impl Into<f32>,
) -> impl Widget<Message, Theme, Renderer> + 'a
where
    Message: 'a,
    Theme: 'a,
    Renderer: 'a,
{
    crate::widgets::text::<Theme>(format!("logo{}", size.into()))
}

/// A value in a range, as iced's `slider(range, value, on_change)`.
pub struct Slider<T, Message> {
    range: std::ops::RangeInclusive<T>,
    value: T,
    default: Option<T>,
    on_change: Box<dyn Fn(T) -> Message>,
}

/// A slider over `range` at `value`.
pub fn slider<T, Message>(
    range: std::ops::RangeInclusive<T>,
    value: T,
    on_change: impl Fn(T) -> Message + 'static,
) -> Slider<T, Message>
where
    T: Copy + From<u8> + PartialOrd,
{
    Slider {
        range,
        value,
        default: None,
        on_change: Box::new(on_change),
    }
}

impl<T: Copy, Message> Slider<T, Message> {
    /// The value a reset gives.
    pub fn default(mut self, default: impl Into<T>) -> Self {
        self.default = Some(default.into());
        self
    }
}

impl<T: Copy + Debug, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Slider<T, Message>
{
    fn labels(&self, labels: &mut Vec<String>) {
        labels.push(format!(
            "{:?}..={:?}@{:?}/{:?}",
            self.range.start(),
            self.range.end(),
            self.value,
            self.default
        ));
    }

    fn press(&self, label: &str) -> Option<Message> {
        (label == "slide").then(|| (self.on_change)(*self.range.end()))
    }
}

/// A part of a turn, as iced's `Radians`, whose range is an associated
/// constant of a range of itself.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Turn(pub f32);

impl Turn {
    /// A whole turn.
    pub const RANGE: std::ops::RangeInclusive<Self> = Self(0.0)..=Self(1.0);
}

impl From<u8> for Turn {
    fn from(value: u8) -> Self {
        Self(f32::from(value))
    }
}

/// Where a hint goes, which the crate's `Default` puts on top.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub enum Position {
    /// Above.
    #[default]
    Top,
    /// Below.
    Bottom,
}

/// The application logic of a program, of a state and a message.
pub trait Program {
    /// The state.
    type State;
    /// The message.
    type Message: Debug;
    /// The state the program starts from, and the work it asks for.
    fn boot(&self) -> (Self::State, crate::Task<Self::Message>);
    /// Change the state for the message, and give the message the work it
    /// asks for gives, if any.
    fn update(&self, state: &mut Self::State, message: Self::Message) -> Option<Self::Message>;
    /// The labels showing the state, and the message a press on `label`
    /// sends.
    fn view(
        &self,
        state: &Self::State,
        label: &str,
        labels: &mut Vec<String>,
    ) -> Option<Self::Message>;
    /// The messages the state listens for.
    fn subscription(&self, _state: &Self::State) -> Subscription<Self::Message> {
        Subscription::none()
    }
    /// The theme of the state, if the program sets one.
    fn theme(&self, _state: &Self::State) -> Option<Theme> {
        None
    }
    /// The scale factor of the state.
    fn scale_factor(&self, _state: &Self::State) -> f32 {
        1.0
    }
    /// The window's title for the state, if the program sets one.
    fn title(&self, _state: &Self::State) -> Option<String> {
        None
    }
}

/// The title of a program's window, as iced's `Title<State>`: a string
/// that lives for the program, or any `Fn(&State) -> String`.
pub trait Title<State> {
    /// The title for the state.
    fn title(&self, state: &State) -> String;
}

impl<State> Title<State> for &'static str {
    fn title(&self, _state: &State) -> String {
        self.to_string()
    }
}

impl<T, State> Title<State> for T
where
    T: Fn(&State) -> String,
{
    fn title(&self, state: &State) -> String {
        self(state)
    }
}

/// A font named by a string that lives for the program, as iced's
/// `Font::with_name(&'static str)`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Named {
    name: &'static str,
}

impl Named {
    /// The font named `name`.
    pub fn with_name(name: &'static str) -> Self {
        Self { name }
    }

    /// Its name's length.
    pub fn len(&self) -> usize {
        self.name.len()
    }
}

/// A heading, which only a string that lives for the program is.
pub trait Heading {
    /// Its text.
    fn text(&self) -> &'static str;
}

impl Heading for &'static str {
    fn text(&self) -> &'static str {
        self
    }
}

/// The text of `heading`.
pub fn heading(heading: impl Heading) -> &'static str {
    heading.text()
}

/// `program` with the title `title` gives its state.
struct WithTitle<P, F> {
    program: P,
    title: F,
}

impl<P, F> Program for WithTitle<P, F>
where
    P: Program,
    F: Title<P::State>,
{
    type State = P::State;
    type Message = P::Message;

    fn boot(&self) -> (Self::State, crate::Task<Self::Message>) {
        self.program.boot()
    }

    fn update(&self, state: &mut Self::State, message: Self::Message) -> Option<Self::Message> {
        self.program.update(state, message)
    }

    fn view(
        &self,
        state: &Self::State,
        label: &str,
        labels: &mut Vec<String>,
    ) -> Option<Self::Message> {
        self.program.view(state, label, labels)
    }

    fn subscription(&self, state: &Self::State) -> Subscription<Self::Message> {
        self.program.subscription(state)
    }

    fn theme(&self, state: &Self::State) -> Option<Theme> {
        self.program.theme(state)
    }

    fn scale_factor(&self, state: &Self::State) -> f32 {
        self.program.scale_factor(state)
    }

    fn title(&self, state: &Self::State) -> Option<String> {
        Some(self.title.title(state))
    }
}

/// The look of a program, as iced's `Theme`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Theme {
    /// Light.
    Light,
    /// Dark.
    Dark,
}

/// The theme logic of a program, as iced's `ThemeFn`: the theme itself,
/// or any `Fn(&State)` whose output turns into an optional theme.
pub trait ThemeFn<State, Theme> {
    /// The theme of the state.
    fn theme(&self, state: &State) -> Option<Theme>;
}

impl<State> ThemeFn<State, Theme> for Theme {
    fn theme(&self, _state: &State) -> Option<Theme> {
        Some(*self)
    }
}

impl<F, T, State, Theme> ThemeFn<State, Theme> for F
where
    F: Fn(&State) -> T,
    T: Into<Option<Theme>>,
{
    fn theme(&self, state: &State) -> Option<Theme> {
        (self)(state).into()
    }
}

/// `program` with the theme `theme` gives its state.
struct WithTheme<P, F> {
    program: P,
    theme: F,
}

impl<P, F> Program for WithTheme<P, F>
where
    P: Program,
    F: ThemeFn<P::State, Theme>,
{
    type State = P::State;
    type Message = P::Message;

    fn boot(&self) -> (Self::State, crate::Task<Self::Message>) {
        self.program.boot()
    }

    fn update(&self, state: &mut Self::State, message: Self::Message) -> Option<Self::Message> {
        self.program.update(state, message)
    }

    fn view(
        &self,
        state: &Self::State,
        label: &str,
        labels: &mut Vec<String>,
    ) -> Option<Self::Message> {
        self.program.view(state, label, labels)
    }

    fn subscription(&self, state: &Self::State) -> Subscription<Self::Message> {
        self.program.subscription(state)
    }

    fn theme(&self, state: &Self::State) -> Option<Theme> {
        self.theme.theme(state)
    }

    fn title(&self, state: &Self::State) -> Option<String> {
        self.program.title(state)
    }
}

/// `program` at the scale factor `scale_factor` gives its state.
struct WithScaleFactor<P, F> {
    program: P,
    scale_factor: F,
}

impl<P, F> Program for WithScaleFactor<P, F>
where
    P: Program,
    F: Fn(&P::State) -> f32,
{
    type State = P::State;
    type Message = P::Message;

    fn boot(&self) -> (Self::State, crate::Task<Self::Message>) {
        self.program.boot()
    }

    fn update(&self, state: &mut Self::State, message: Self::Message) -> Option<Self::Message> {
        self.program.update(state, message)
    }

    fn view(
        &self,
        state: &Self::State,
        label: &str,
        labels: &mut Vec<String>,
    ) -> Option<Self::Message> {
        self.program.view(state, label, labels)
    }

    fn subscription(&self, state: &Self::State) -> Subscription<Self::Message> {
        self.program.subscription(state)
    }

    fn theme(&self, state: &Self::State) -> Option<Theme> {
        self.program.theme(state)
    }

    fn scale_factor(&self, state: &Self::State) -> f32 {
        (self.scale_factor)(state)
    }

    fn title(&self, state: &Self::State) -> Option<String> {
        self.program.title(state)
    }
}

/// The messages a program listens for, as iced's `Subscription`.
pub struct Subscription<T> {
    messages: Vec<T>,
}

impl<T> Subscription<T> {
    /// No messages.
    pub fn none() -> Self {
        Subscription {
            messages: Vec::new(),
        }
    }

    /// The subscription with each message turned into another by `f`.
    pub fn map<A>(self, f: impl Fn(T) -> A + 'static) -> Subscription<A> {
        Subscription {
            messages: self.messages.into_iter().map(f).collect(),
        }
    }
}

/// An event a program may listen for, as iced's `Event`.
#[derive(Debug, Clone, Copy)]
pub enum Signal {
    /// The window's scale factor changed.
    Rescaled(f32),
    /// Something else happened.
    Moved,
}

/// The messages `f` makes of each event, as iced's
/// `event::listen_with(fn(Event, Status, window::Id) -> Option<Message>)`
/// takes a function pointer, which iced calls on another thread.
pub fn listen_with<Message>(f: fn(Signal, bool, u32) -> Option<Message>) -> Subscription<Message>
where
    Message: crate::MaybeSend + 'static,
{
    Subscription {
        messages: std::thread::spawn(move || {
            [Signal::Moved, Signal::Rescaled(2.0)]
                .into_iter()
                .filter_map(|signal| f(signal, false, 1))
                .collect()
        })
        .join()
        .expect("the listening thread finishes"),
    }
}

/// The first `count` ticks of a clock, as iced's `time::every`.
pub fn ticks(count: u32) -> Subscription<u32> {
    Subscription {
        messages: (1..=count).collect(),
    }
}

/// Time, as iced's `time`: std's `Duration` and `Instant` re-exported
/// beside functions that take and give them.
pub mod time {
    pub use std::time::{Duration, Instant};

    use super::Subscription;

    /// `millis` milliseconds, as iced's `time::milliseconds`.
    pub fn milliseconds(millis: u64) -> Duration {
        Duration::from_millis(millis)
    }

    /// The first three ticks of a clock every `duration`, each the instant
    /// it ticks at, as iced's `time::every`.
    pub fn every(duration: Duration) -> Subscription<Instant> {
        let start = Instant::now();
        Subscription {
            messages: (1..=3).map(|tick| start + duration * tick).collect(),
        }
    }
}

/// `program` with the subscription `subscription` of its state.
struct WithSubscription<P, F> {
    program: P,
    subscription: F,
}

impl<P, F> Program for WithSubscription<P, F>
where
    P: Program,
    F: Fn(&P::State) -> Subscription<P::Message>,
{
    type State = P::State;
    type Message = P::Message;

    fn boot(&self) -> (Self::State, crate::Task<Self::Message>) {
        self.program.boot()
    }

    fn update(&self, state: &mut Self::State, message: Self::Message) -> Option<Self::Message> {
        self.program.update(state, message)
    }

    fn view(
        &self,
        state: &Self::State,
        label: &str,
        labels: &mut Vec<String>,
    ) -> Option<Self::Message> {
        self.program.view(state, label, labels)
    }

    fn subscription(&self, state: &Self::State) -> Subscription<Self::Message> {
        (self.subscription)(state)
    }

    fn title(&self, state: &Self::State) -> Option<String> {
        self.program.title(state)
    }
}

/// What makes a program's state and its first work, as iced's `BootFn`:
/// any `Fn()` whose output turns into them.
pub trait BootFn<State, Message> {
    /// The state, and the work it asks for.
    fn boot(&self) -> (State, crate::Task<Message>);
}

impl<T, C, State, Message> BootFn<State, Message> for T
where
    T: Fn() -> C,
    C: IntoBoot<State, Message>,
{
    fn boot(&self) -> (State, crate::Task<Message>) {
        self().into_boot()
    }
}

/// What turns into a program's state and its first work, as iced's
/// `IntoBoot`: the state alone, or the state and a task.
pub trait IntoBoot<State, Message> {
    /// The state, and the work it asks for.
    fn into_boot(self) -> (State, crate::Task<Message>);
}

impl<State, Message> IntoBoot<State, Message> for State {
    fn into_boot(self) -> (State, crate::Task<Message>) {
        (self, crate::Task::none())
    }
}

impl<State, Message> IntoBoot<State, Message> for (State, crate::Task<Message>) {
    fn into_boot(self) -> (State, crate::Task<Message>) {
        self
    }
}

/// An application, which a program the caller can't name carries.
pub struct Application<P: Program> {
    program: P,
    fonts: Vec<Cow<'static, [u8]>>,
}

/// The application of `boot`, `update` and `view`, as iced's
/// `application(..)`.
pub fn application<State, Message, Theme, Renderer>(
    boot: impl BootFn<State, Message>,
    update: impl crate::UpdateFn<State, Message>,
    view: impl for<'a> crate::widgets::ViewFn<'a, State, Message, Theme, Renderer>,
) -> Application<impl Program<State = State, Message = Message>>
where
    State: 'static,
    Message: Debug + 'static,
    Theme: 'static,
    Renderer: 'static,
{
    struct Instance<State, Message, Theme, Renderer, B, U, V> {
        boot: B,
        update: U,
        view: V,
        types: std::marker::PhantomData<(State, Message, Theme, Renderer)>,
    }

    impl<State, Message, Theme, Renderer, B, U, V> Program
        for Instance<State, Message, Theme, Renderer, B, U, V>
    where
        Message: Debug,
        B: BootFn<State, Message>,
        U: crate::UpdateFn<State, Message>,
        V: for<'a> crate::widgets::ViewFn<'a, State, Message, Theme, Renderer>,
    {
        type State = State;
        type Message = Message;

        fn boot(&self) -> (State, crate::Task<Message>) {
            self.boot.boot()
        }

        fn update(&self, state: &mut State, message: Message) -> Option<Message> {
            self.update.update(state, message).0
        }

        fn view(&self, state: &State, label: &str, labels: &mut Vec<String>) -> Option<Message> {
            let widget = self.view.view(state);
            widget.labels(labels);
            widget.press(label)
        }
    }

    Application {
        program: Instance {
            boot,
            update,
            view,
            types: std::marker::PhantomData,
        },
        fonts: Vec::new(),
    }
}

impl<P: Program> Application<P> {
    /// The application with the subscription `f` of its state, as iced's
    /// `Application::subscription`: its program is another one the caller
    /// can't name, of the same state and message.
    pub fn subscription(
        self,
        f: impl Fn(&P::State) -> Subscription<P::Message>,
    ) -> Application<impl Program<State = P::State, Message = P::Message>> {
        Application {
            program: WithSubscription {
                program: self.program,
                subscription: f,
            },
            fonts: self.fonts,
        }
    }

    /// The application with the theme `f` gives its state, as iced's
    /// `Application::theme`: a theme, or a function of the state.
    pub fn theme(
        self,
        f: impl ThemeFn<P::State, Theme>,
    ) -> Application<impl Program<State = P::State, Message = P::Message>> {
        Application {
            program: WithTheme {
                program: self.program,
                theme: f,
            },
            fonts: self.fonts,
        }
    }

    /// The application at the scale factor `f` gives its state, as iced's
    /// `Application::scale_factor`.
    pub fn scale_factor(
        self,
        f: impl Fn(&P::State) -> f32,
    ) -> Application<impl Program<State = P::State, Message = P::Message>> {
        Application {
            program: WithScaleFactor {
                program: self.program,
                scale_factor: f,
            },
            fonts: self.fonts,
        }
    }

    /// The application with the title `title` gives its state, as iced's
    /// `Application::title`: a string that lives for the program, or a
    /// function of the state.
    pub fn title(
        self,
        title: impl Title<P::State>,
    ) -> Application<impl Program<State = P::State, Message = P::Message>> {
        Application {
            program: WithTitle {
                program: self.program,
                title,
            },
            fonts: self.fonts,
        }
    }

    /// The application with `fonts` loaded.
    pub fn fonts(mut self, fonts: impl IntoIterator<Item = impl Into<Cow<'static, [u8]>>>) -> Self {
        self.fonts.extend(fonts.into_iter().map(Into::into));
        self
    }

    /// Show the view and press each label of `EVENTS_PRESS` (comma
    /// separated) in turn, then print the last view.
    pub fn run(self) {
        println!("title {}", std::any::type_name::<P::State>());
        println!(
            "fonts {}",
            self.fonts.iter().map(|font| font.len()).sum::<usize>()
        );
        let presses = std::env::var("EVENTS_PRESS").unwrap_or_default();
        let (mut state, boot) = self.program.boot();
        // Each message the booted state listens for, before any press.
        // A message the work an update asks for gives, delivered after it.
        let deliver = |state: &mut P::State, message: P::Message| {
            let mut next = self.program.update(state, message);
            while let Some(message) = next {
                println!("task message {message:?}");
                next = self.program.update(state, message);
            }
        };
        // The message the boot's work gives, before anything else.
        if let Some(message) = boot.0 {
            println!("boot message {message:?}");
            deliver(&mut state, message);
        }
        for message in self.program.subscription(&state).messages {
            println!("heard {message:?}");
            deliver(&mut state, message);
        }
        for (event, label) in presses
            .split(',')
            .filter(|label| !label.is_empty())
            .enumerate()
        {
            let mut labels = Vec::new();
            let pressed = self.program.view(&state, label, &mut labels);
            println!("{} view {}", event + 1, labels.join(" "));
            if let Some(theme) = self.program.theme(&state) {
                println!("{} theme {theme:?}", event + 1);
            }
            let scale = self.program.scale_factor(&state);
            if scale != 1.0 {
                println!("{} scale {scale}", event + 1);
            }
            if let Some(message) = pressed {
                println!("{} message {message:?}", event + 1);
                deliver(&mut state, message);
            }
        }
        let mut labels = Vec::new();
        let _ = self.program.view(&state, "", &mut labels);
        println!("last view {}", labels.join(" "));
        if let Some(title) = self.program.title(&state) {
            println!("named {title}");
        }
    }
}

/// A choice among options, as iced's `pick_list(selected, options,
/// to_string)`: the options are any `Borrow<[T]>`, and the selected one any
/// `Borrow<T>`, which a script value fills.
pub struct Pick<'a, T, L, V, Message> {
    selected: Option<V>,
    options: L,
    to_string: Box<dyn Fn(&T) -> String + 'a>,
    on_select: Option<Box<dyn Fn(T) -> Message + 'a>>,
    placeholder: String,
}

/// A choice of `options`, `selected` marked, each shown by `to_string`.
pub fn pick<'a, T, L, V, Message>(
    selected: Option<V>,
    options: L,
    to_string: impl Fn(&T) -> String + 'a,
) -> Pick<'a, T, L, V, Message>
where
    T: PartialEq + Clone + 'a,
    L: std::borrow::Borrow<[T]> + 'a,
    V: std::borrow::Borrow<T> + 'a,
{
    Pick {
        selected,
        options,
        to_string: Box::new(to_string),
        on_select: None,
        placeholder: String::new(),
    }
}

impl<'a, T, L, V, Message> Pick<'a, T, L, V, Message>
where
    T: PartialEq + Clone + 'a,
    L: std::borrow::Borrow<[T]> + 'a,
    V: std::borrow::Borrow<T> + 'a,
{
    /// The message a choice sends.
    pub fn on_select(mut self, on_select: impl Fn(T) -> Message + 'a) -> Self {
        self.on_select = Some(Box::new(on_select));
        self
    }

    /// What shows while nothing is selected.
    pub fn placeholder(mut self, placeholder: impl Into<String>) -> Self {
        self.placeholder = placeholder.into();
        self
    }
}

impl<'a, T, L, V, Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for Pick<'a, T, L, V, Message>
where
    T: PartialEq + Clone + 'a,
    L: std::borrow::Borrow<[T]> + 'a,
    V: std::borrow::Borrow<T> + 'a,
{
    fn labels(&self, labels: &mut Vec<String>) {
        let selected = self.selected.as_ref().map(std::borrow::Borrow::borrow);
        let options = self
            .options
            .borrow()
            .iter()
            .map(|option| {
                let mark = if Some(option) == selected { "*" } else { "" };
                format!("{mark}{}", (self.to_string)(option))
            })
            .collect::<Vec<_>>();
        let shown = selected.map_or_else(
            || self.placeholder.clone(),
            |selected| (self.to_string)(selected),
        );
        labels.push(format!("<{shown}: {}>", options.join("|")));
    }

    fn press(&self, label: &str) -> Option<Message> {
        let on_select = self.on_select.as_ref()?;
        self.options
            .borrow()
            .iter()
            .find(|option| (self.to_string)(option) == label)
            .map(|option| on_select(option.clone()))
    }
}

/// A string literal is a widget, as iced's `impl Widget for &str` makes
/// `column!["label", ..]` write `Widget::_boxed("label")`.
impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for &str {
    fn labels(&self, labels: &mut Vec<String>) {
        labels.push(format!("'{self}'"));
    }

    fn press(&self, _label: &str) -> Option<Message> {
        None
    }
}

mod caption_core {
    /// Text at a theme, which no public path names, as iced's
    /// `core::widget::Text<'a, Theme>` while `iced::advanced` is off.
    pub struct Caption<'a, Theme> {
        pub(crate) text: String,
        pub(crate) look: Option<super::Look>,
        pub(crate) theme: std::marker::PhantomData<&'a Theme>,
    }
}

/// Text at the crate's theme: an alias that passes its parameters on, and
/// the only name of its type, as iced's `widget::Text` is.
pub type Caption<'a, Theme = crate::Theme> = caption_core::Caption<'a, Theme>;

/// A caption showing `value`, as iced's `text(..)`, whose name a macro
/// and the function share.
pub fn caption<'a, Theme>(value: impl ToString) -> Caption<'a, Theme> {
    caption_core::Caption {
        text: value.to_string(),
        look: None,
        theme: std::marker::PhantomData,
    }
}

impl<Theme> caption_core::Caption<'_, Theme> {
    /// A caption showing `value`, as iced's `Text::new`.
    pub fn new(value: impl ToString) -> Self {
        caption(value)
    }

    /// The caption in `look`.
    pub fn look(mut self, look: Look) -> Self {
        self.look = Some(look);
        self
    }
}

/// A caption of a format string, as iced's `text!`.
#[macro_export]
macro_rules! controls_caption {
    ($($arg:tt)*) => {
        $crate::controls::Caption::new(format!($($arg)*))
    };
}

pub use crate::controls_caption as caption;

/// The caption's module, as iced's `widget::text` module: a module, a
/// macro and a function share the name `caption`, each in its namespace.
pub mod caption {
    pub use super::Caption;
}

/// Text a widget borrows, as iced's `Text<'a>` borrows what `text(impl
/// IntoFragment<'a>)` takes.
pub struct Quote<'a, Theme> {
    text: &'a str,
    size: u32,
    theme: std::marker::PhantomData<Theme>,
}

/// A quote of `text`, which it borrows.
pub fn quote<'a, Theme>(text: impl IntoLabel<'a>) -> Quote<'a, Theme> {
    Quote {
        text: text.into_label(),
        size: 16,
        theme: std::marker::PhantomData,
    }
}

impl<Theme> Quote<'_, Theme> {
    /// The quote at `size`.
    pub fn size(mut self, size: u32) -> Self {
        self.size = size;
        self
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Quote<'_, Theme> {
    fn labels(&self, labels: &mut Vec<String>) {
        labels.push(format!("'{}'@{}", self.text, self.size));
    }

    fn press(&self, _label: &str) -> Option<Message> {
        None
    }
}

impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer>
    for caption_core::Caption<'_, Theme>
{
    fn labels(&self, labels: &mut Vec<String>) {
        match &self.look {
            Some(look) => labels.push(format!("\"{}\"/{}", self.text, look.name())),
            None => labels.push(format!("\"{}\"", self.text)),
        }
    }

    fn press(&self, _label: &str) -> Option<Message> {
        None
    }
}

/// A tone and whether it is monospaced, with constants of no scalar value,
/// as iced's `Font::MONOSPACE` and `Theme::ALL`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Look {
    /// Its tone.
    pub tone: u8,
    /// Whether it is monospaced.
    pub mono: bool,
}

impl Look {
    /// The plain look.
    pub const DEFAULT: Look = Look {
        tone: 0,
        mono: false,
    };

    /// The monospaced look.
    pub const MONOSPACE: Look = Look {
        mono: true,
        ..Self::DEFAULT
    };

    /// Every look.
    pub const ALL: &'static [Self] = &[Self::DEFAULT, Self::MONOSPACE];

    /// Its name.
    pub fn name(&self) -> String {
        format!("{}{}", if self.mono { "mono" } else { "plain" }, self.tone)
    }
}

/// A code's cells, as iced's `qr_code` module has them: data that isn't
/// `Clone` and keeps a count of its draws in a `Cell`, as iced's keeps its
/// canvas cache, so a widget borrows it and a copy would count from zero.
pub mod qr_code {
    use crate::widgets::Widget;

    /// The cells of a code, as iced's `qr_code::Data`.
    #[derive(Debug)]
    pub struct Data {
        width: usize,
        draws: std::cell::Cell<u32>,
    }

    /// Why a code can't hold some text.
    #[derive(Debug, Clone, Copy)]
    pub enum Error {
        /// There is no text.
        Empty,
    }

    impl Data {
        /// The code of `text`.
        pub fn new(text: &str) -> Result<Self, Error> {
            if text.is_empty() {
                return Err(Error::Empty);
            }
            Ok(Self {
                width: 2 * text.len() + 1,
                draws: std::cell::Cell::new(0),
            })
        }

        /// The code with a border of cells around it.
        pub fn grow(&mut self) {
            self.width += 2;
        }
    }

    /// A widget that borrows a code's data, as iced's `QRCode<'a, Theme>`:
    /// no renderer parameter, and the theme defaults to the crate's.
    pub struct QRCode<'a, Theme = crate::Theme> {
        data: &'a Data,
        cell_size: f32,
        theme: std::marker::PhantomData<Theme>,
    }

    impl<'a, Theme> QRCode<'a, Theme> {
        /// The widget of `data`, which it borrows.
        pub fn new(data: &'a Data) -> Self {
            Self {
                data,
                cell_size: 4.0,
                theme: std::marker::PhantomData,
            }
        }

        /// The size of one cell.
        pub fn cell_size(mut self, cell_size: f32) -> Self {
            self.cell_size = cell_size;
            self
        }

        /// The size of the whole code, with a quiet zone of one cell.
        pub fn total_size(mut self, total_size: f32) -> Self {
            self.cell_size = total_size / (self.data.width + 2) as f32;
            self
        }
    }

    impl<Message, Theme, Renderer> Widget<Message, Theme, Renderer> for QRCode<'_, Theme> {
        fn labels(&self, labels: &mut Vec<String>) {
            let draws = self.data.draws.get() + 1;
            self.data.draws.set(draws);
            labels.push(format!(
                "qr {}@{} draw {draws}",
                self.data.width, self.cell_size
            ));
        }

        fn press(&self, _label: &str) -> Option<Message> {
            None
        }
    }
}

/// The widget of a code's `data`, which it borrows for `'a`, as iced's
/// `qr_code(data)`: the module beside it shares its name.
pub fn qr_code<'a, Theme>(data: &'a qr_code::Data) -> qr_code::QRCode<'a, Theme>
where
    Theme: 'a,
{
    qr_code::QRCode::new(data)
}

/// A searchable choice, as iced's `combo_box`.
pub mod choose {
    use crate::widgets::Widget;
    use std::fmt::Display;

    /// The options to choose from, as iced's `combo_box::State`.
    pub struct State<T> {
        options: Vec<T>,
    }

    impl<T: Display + Clone> State<T> {
        /// The state of `options`.
        pub fn new(options: Vec<T>) -> Self {
            Self { options }
        }
    }

    /// A choice that borrows its state, as iced's `ComboBox`.
    pub struct Choose<'a, T, Message, Theme = crate::Theme> {
        pub(super) state: &'a State<T>,
        pub(super) placeholder: &'a str,
        pub(super) selection: Option<String>,
        pub(super) on_selected: Box<dyn Fn(T) -> Message + 'a>,
        pub(super) theme: std::marker::PhantomData<Theme>,
    }

    impl<T, Message, Theme, Renderer> Widget<Message, Theme, Renderer> for Choose<'_, T, Message, Theme>
    where
        T: Display + Clone,
    {
        fn labels(&self, labels: &mut Vec<String>) {
            let options = self
                .state
                .options
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>();
            let shown = self.selection.as_deref().unwrap_or(self.placeholder);
            labels.push(format!("<{shown}? {}>", options.join("|")));
        }

        fn press(&self, label: &str) -> Option<Message> {
            self.state
                .options
                .iter()
                .find(|option| option.to_string() == label)
                .map(|option| (self.on_selected)(option.clone()))
        }
    }
}

/// A choice among `state`'s options, `selection` shown, as iced's
/// `combo_box(state, placeholder, selection, on_selected)`: the selection
/// is borrowed for the call alone, at a late-bound lifetime.
pub fn choose<'a, T, Message, Theme>(
    state: &'a choose::State<T>,
    placeholder: impl IntoLabel<'a>,
    selection: Option<&T>,
    on_selected: impl Fn(T) -> Message + 'a,
) -> choose::Choose<'a, T, Message, Theme>
where
    T: std::fmt::Display + Clone,
    Theme: 'a,
{
    choose::Choose {
        state,
        placeholder: placeholder.into_label(),
        selection: selection.map(ToString::to_string),
        on_selected: Box::new(on_selected),
        theme: std::marker::PhantomData,
    }
}

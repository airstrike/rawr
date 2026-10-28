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
    /// The state the program starts from.
    fn boot(&self) -> Self::State;
    /// Change the state for the message.
    fn update(&self, state: &mut Self::State, message: Self::Message);
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

/// The first `count` ticks of a clock, as iced's `time::every`.
pub fn ticks(count: u32) -> Subscription<u32> {
    Subscription {
        messages: (1..=count).collect(),
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

    fn boot(&self) -> Self::State {
        self.program.boot()
    }

    fn update(&self, state: &mut Self::State, message: Self::Message) {
        self.program.update(state, message);
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
}

/// What makes a program's state: any `Fn()` whose output turns into it.
pub trait BootFn<State> {
    /// The state.
    fn boot(&self) -> State;
}

impl<T, C, State> BootFn<State> for T
where
    T: Fn() -> C,
    C: Into<State>,
{
    fn boot(&self) -> State {
        self().into()
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
    boot: impl BootFn<State>,
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
        B: BootFn<State>,
        U: crate::UpdateFn<State, Message>,
        V: for<'a> crate::widgets::ViewFn<'a, State, Message, Theme, Renderer>,
    {
        type State = State;
        type Message = Message;

        fn boot(&self) -> State {
            self.boot.boot()
        }

        fn update(&self, state: &mut State, message: Message) {
            let _task = self.update.update(state, message);
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
        let mut state = self.program.boot();
        // Each message the booted state listens for, before any press.
        for message in self.program.subscription(&state).messages {
            println!("heard {message:?}");
            self.program.update(&mut state, message);
        }
        for (event, label) in presses
            .split(',')
            .filter(|label| !label.is_empty())
            .enumerate()
        {
            let mut labels = Vec::new();
            let pressed = self.program.view(&state, label, &mut labels);
            println!("{} view {}", event + 1, labels.join(" "));
            if let Some(message) = pressed {
                println!("{} message {message:?}", event + 1);
                self.program.update(&mut state, message);
            }
        }
        let mut labels = Vec::new();
        let _ = self.program.view(&state, "", &mut labels);
        println!("last view {}", labels.join(" "));
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

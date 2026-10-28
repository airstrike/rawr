//! Widgets in the shape of iced's checkbox, slider and tooltip examples:
//! an `Option` of a callable, a crate function where a style closure goes,
//! a widget invariant in the lifetime of the label it borrows, an opaque
//! `impl Widget` a function gives, an application an opaque `impl Program`
//! carries, and a field of a type whose `Default` the crate derives.

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

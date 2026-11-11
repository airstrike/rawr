//! A view driven without a window, in the shape of iced_test's simulator:
//! `simulator(view)` takes any `impl Widget<Message, Theme, Renderer>`, so
//! the view's own `impl Widget<Message>` fixes the theme and renderer.
//! `click` is generic over a `Selector` the crate implements for `&str` and
//! for any `FnMut(Candidate<'_>)` closure, and `simulate` takes the events
//! of any `IntoIterator`, which the opaque iterator `click()` gives is.

use crate::widgets::Widget;

/// What a selector is offered, as iced's `selector::Candidate<'a>`: it
/// borrows the label of the widget it stands for.
#[derive(Debug, Clone, Copy)]
pub struct Candidate<'a> {
    label: &'a str,
}

impl<'a> Candidate<'a> {
    /// The label of the widget, as it shows it.
    pub fn label(&self) -> &'a str {
        self.label
    }
}

/// A widget found by its text, as iced's `selector::target::Text`.
#[derive(Debug, Clone, PartialEq)]
pub struct Text {
    label: String,
}

/// A target with bounds to point at, as iced's `selector::Bounded`.
pub trait Bounded: std::fmt::Debug {
    /// The label the pointer goes to, when the target is visible.
    fn visible(&self) -> Option<&str>;
}

impl Bounded for Text {
    fn visible(&self) -> Option<&str> {
        Some(&self.label)
    }
}

/// A label a closure selector found, which it points at as it is.
impl Bounded for String {
    fn visible(&self) -> Option<&str> {
        Some(self)
    }
}

/// A way to find a widget, as iced_test's `Selector`.
pub trait Selector {
    /// What the selection gives.
    type Output;

    /// The selection among the widget `candidate` stands for.
    fn select(&mut self, candidate: Candidate<'_>) -> Option<Self::Output>;

    /// What the selector finds, for an error.
    fn description(&self) -> String;
}

/// A widget whose text is the string, as iced_test's `impl Selector for
/// &str`. A button's label shows its text in brackets.
impl Selector for &str {
    type Output = Text;

    fn select(&mut self, candidate: Candidate<'_>) -> Option<Text> {
        let label = candidate
            .label
            .trim_start_matches('[')
            .trim_end_matches(']');
        (label == *self).then(|| Text {
            label: label.to_owned(),
        })
    }

    fn description(&self) -> String {
        format!("text {self:?}")
    }
}

impl<F, T> Selector for F
where
    F: FnMut(Candidate<'_>) -> Option<T>,
{
    type Output = T;

    fn select(&mut self, candidate: Candidate<'_>) -> Option<T> {
        self(candidate)
    }

    fn description(&self) -> String {
        "a closure".to_owned()
    }
}

/// Why a simulation step failed, as iced_test's `Error`.
#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    /// No widget the selector describes is shown.
    NotFound(String),
}

/// An event a simulator takes, as iced's `Event`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Event {
    /// The mouse button went down.
    Pressed,
    /// The mouse button went up.
    Released,
}

/// What a widget did with an event, as iced's `event::Status`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Status {
    /// A widget took it.
    Captured,
    /// None did.
    Ignored,
}

/// A view and the messages its simulated events sent, as iced_test's
/// `Simulator`, whose theme and renderer default to the crate's.
pub struct Simulator<'a, Message, Theme = crate::Theme, Renderer = crate::widgets::Renderer> {
    widget: Box<dyn Widget<Message, Theme, Renderer> + 'a>,
    pointed: Option<String>,
    messages: Vec<Message>,
}

impl<'a, Message, Theme, Renderer> Simulator<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: 'a,
{
    /// A simulator of `element`.
    pub fn new(element: impl Widget<Message, Theme, Renderer> + 'a) -> Self {
        Self {
            widget: Box::new(element),
            pointed: None,
            messages: Vec::new(),
        }
    }

    /// What `selector` selects first among the widgets shown.
    pub fn find<S>(&mut self, mut selector: S) -> Result<S::Output, Error>
    where
        S: Selector + Send,
        S::Output: Clone + Send,
    {
        let mut labels = Vec::new();
        self.widget.labels(&mut labels);
        labels
            .iter()
            .find_map(|label| selector.select(Candidate { label }))
            .ok_or_else(|| Error::NotFound(selector.description()))
    }

    /// Points at the widget labelled `label`.
    pub fn point_at(&mut self, label: &str) {
        self.pointed = Some(label.to_owned());
    }

    /// Clicks what `selector` selects: points at it and simulates a click.
    pub fn click<S>(&mut self, selector: S) -> Result<S::Output, Error>
    where
        S: Selector + Send,
        S::Output: Bounded + Clone + Send + Sync + 'static,
    {
        let target = self.find(selector)?;
        if let Some(label) = target.visible() {
            self.point_at(label);
        }
        let _ = self.simulate(click());
        Ok(target)
    }

    /// Each of `events` at the widget pointed at, and what it did.
    pub fn simulate(&mut self, events: impl IntoIterator<Item = Event>) -> Vec<Status> {
        let mut statuses = Vec::new();
        for event in events {
            let message = match (event, &self.pointed) {
                (Event::Pressed, Some(label)) => self.widget.press(label),
                _ => None,
            };
            statuses.push(if message.is_some() {
                Status::Captured
            } else {
                Status::Ignored
            });
            self.messages.extend(message);
        }
        statuses
    }

    /// The messages the simulated events sent, in order.
    pub fn into_messages(self) -> impl Iterator<Item = Message> + use<Message, Theme, Renderer> {
        self.messages.into_iter()
    }
}

/// A simulator of `element`, as iced_test's `simulator`.
pub fn simulator<'a, Message, Theme, Renderer>(
    element: impl Widget<Message, Theme, Renderer> + 'a,
) -> Simulator<'a, Message, Theme, Renderer>
where
    Message: 'a,
    Theme: 'a,
    Renderer: 'a,
{
    Simulator::new(element)
}

/// The events of a click, as iced_test's `simulator::click`.
pub fn click() -> impl Iterator<Item = Event> {
    [Event::Pressed, Event::Released].into_iter()
}

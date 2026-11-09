//! A stand-in for iced: a theme of its own, markdown settings an app's
//! theme converts into, and traits a widget calls on the app's theme.

/// The crate's own theme, as `iced::Theme`.
#[derive(Debug, Clone, PartialEq)]
pub struct Theme {
    pub name: String,
    pub accent: u32,
}

impl Theme {
    pub fn custom(name: String, accent: u32) -> Self {
        Self { name, accent }
    }
}

/// What a markdown view is drawn with, as `iced::widget::markdown::Settings`.
#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub text_size: f32,
    pub code_size: f32,
}

impl Settings {
    pub fn with_text_size(text_size: f32) -> Self {
        Self {
            text_size,
            code_size: text_size,
        }
    }
}

/// A markdown view, which takes anything that converts into its settings.
pub fn view(text: &str, settings: impl Into<Settings>) -> String {
    let settings = settings.into();
    format!(
        "{text} at {} with code at {}",
        settings.text_size, settings.code_size
    )
}

/// What a chart asks of the app's theme at each draw, as hyozu's `Design`.
pub trait Design {
    fn background(&self) -> u32;
    fn name(&self) -> String;
    fn corners(&self) -> f32 {
        0.0
    }
}

pub fn chart<D: Design>(design: &D) -> String {
    format!(
        "{} chart on {} with corners {}",
        design.name(),
        design.background(),
        design.corners()
    )
}

/// What a widget asks of the app's theme, as iced's `Catalog`s.
pub trait Catalog {
    fn id(&self) -> &str;
}

pub fn catalog<C: Catalog>(catalog: &C) -> String {
    format!("catalog {}", catalog.id())
}

/// What the app's theme names itself by, as `iced::theme::Base`.
pub trait Base {
    fn name(&self) -> &str;
}

pub fn base<B: Base>(base: &B) -> String {
    format!("base {}", base.name())
}

/// A widget of any kind, as `iced::Element<'a, Message, Theme, Renderer>`.
pub struct Element<'a, Message, T> {
    label: Label<'a, T>,
    message: Option<Message>,
}

impl<M, T> Element<'_, M, T> {
    pub fn draw(&self, theme: &T) -> String {
        let pressable = if self.message.is_some() { "" } else { " idle" };
        format!("{}{pressable}", self.label.draw(theme))
    }

    pub fn message(&self) -> Option<&M> {
        self.message.as_ref()
    }
}

impl<'a, M, T> Element<'a, M, T> {
    /// The element with the message it gives mapped by `f`, as
    /// `iced::Element::map`.
    pub fn map<B>(self, f: impl Fn(M) -> B + 'a) -> Element<'a, B, T> {
        Element {
            label: self.label,
            message: self.message.map(f),
        }
    }
}

/// A widget that keeps the app's theme borrowed, as an iced `Element<'a>`
/// keeps what its view borrows.
pub struct Badge<'a, T> {
    text: String,
    theme: &'a T,
}

pub fn badge<'a, T: Base>(text: &str, theme: &'a T) -> Badge<'a, T> {
    Badge {
        text: text.to_owned(),
        theme,
    }
}

impl<T: Base> Badge<'_, T> {
    pub fn draw(&self) -> String {
        format!("{} badge on {}", self.text, self.theme.name())
    }
}

/// A widget styled by a function of the app's theme, as an iced widget's
/// `style`.
pub struct Label<'a, T> {
    content: String,
    style: Box<dyn Fn(&T) -> u32 + 'a>,
}

pub fn label<'a, T>(content: &str) -> Label<'a, T> {
    Label {
        content: content.to_owned(),
        style: Box::new(|_| 0),
    }
}

impl<'a, T> Label<'a, T> {
    pub fn style(mut self, style: impl Fn(&T) -> u32 + 'a) -> Self {
        self.style = Box::new(style);
        self
    }

    pub fn draw(&self, theme: &T) -> String {
        format!("{} styled {}", self.content, (self.style)(theme))
    }

    pub fn boxed<M>(self) -> Element<'a, M, T> {
        Element {
            label: self,
            message: None,
        }
    }

    pub fn on_press<M>(self, message: M) -> Element<'a, M, T> {
        Element {
            label: self,
            message: Some(message),
        }
    }
}

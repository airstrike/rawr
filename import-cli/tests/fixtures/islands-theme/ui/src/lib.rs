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

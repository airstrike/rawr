//! A stand-in for iced: a theme of its own, markdown settings an app's
//! theme converts into, traits a widget calls on the app's theme, and the
//! iced fork's generic widgets over it.
use std::marker::PhantomData;

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

/// The crate's renderer, as `iced::Renderer`.
#[derive(Debug, Default)]
pub struct Renderer;

/// Anything drawn over the app's theme, as the iced fork's `Widget`, whose
/// `map` and `boxed` give a `Map` and an `Element`.
pub trait Widget<M, T, R = Renderer> {
    fn draw(&self, theme: &T) -> String;

    /// The message a press gives, if the widget takes presses.
    fn press(&self) -> Option<M> {
        None
    }

    fn map<F, B>(self, mapper: F) -> Map<Self, F, M>
    where
        Self: Sized,
        F: Fn(M) -> B,
    {
        Map {
            widget: self,
            mapper,
            input: PhantomData,
        }
    }

    fn boxed<'a>(self) -> Element<'a, M, T, R>
    where
        Self: Sized + 'a,
    {
        Element {
            widget: Box::new(self),
        }
    }
}

/// A widget whose messages are mapped, as the iced fork's `widget::Map`.
pub struct Map<W, F, A> {
    widget: W,
    mapper: F,
    input: PhantomData<A>,
}

impl<W, F, A, B, T, R> Widget<B, T, R> for Map<W, F, A>
where
    W: Widget<A, T, R>,
    F: Fn(A) -> B,
{
    fn draw(&self, theme: &T) -> String {
        self.widget.draw(theme)
    }

    fn press(&self) -> Option<B> {
        self.widget.press().map(&self.mapper)
    }
}

/// A widget of any kind, as `iced::Element<'a, Message, Theme, Renderer>`.
pub struct Element<'a, M, T, R = Renderer> {
    widget: Box<dyn Widget<M, T, R> + 'a>,
}

impl<M, T, R> Element<'_, M, T, R> {
    pub fn draw(&self, theme: &T) -> String {
        let pressable = if self.widget.press().is_some() {
            ""
        } else {
            " idle"
        };
        format!("{}{pressable}", self.widget.draw(theme))
    }

    pub fn message(&self) -> Option<M> {
        self.widget.press()
    }
}

impl<M, T, R> Widget<M, T, R> for Element<'_, M, T, R> {
    fn draw(&self, theme: &T) -> String {
        self.widget.draw(theme)
    }

    fn press(&self) -> Option<M> {
        self.widget.press()
    }
}

/// A widget's length, as `iced::Length`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Length {
    Fill,
    Shrink,
    Fixed(f32),
    Bounded { max: f32 },
}

pub use Length::Fill;

impl Length {
    /// The length bounded by `max` pixels, as `iced::Length::max`.
    pub fn max(self, max: impl Into<Pixels>) -> Self {
        let max = max.into().0;
        match self {
            Length::Fixed(_) => self,
            Length::Fill | Length::Shrink | Length::Bounded { .. } => Length::Bounded { max },
        }
    }
}

impl From<f32> for Length {
    fn from(amount: f32) -> Self {
        Length::Fixed(amount)
    }
}

impl From<u32> for Length {
    fn from(amount: u32) -> Self {
        Length::Fixed(amount as f32)
    }
}

/// A distance in pixels, as `iced::Pixels`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pixels(pub f32);

impl From<f32> for Pixels {
    fn from(amount: f32) -> Self {
        Pixels(amount)
    }
}

/// Children laid out top to bottom, as the iced fork's `Column<W>`.
pub struct Column<W> {
    children: Vec<W>,
    spacing: u32,
    width: Length,
}

impl<W> Column<W> {
    pub fn with_children(children: impl IntoIterator<Item = W>) -> Self {
        Self {
            children: children.into_iter().collect(),
            spacing: 0,
            width: Length::Shrink,
        }
    }

    pub fn spacing(mut self, spacing: u32) -> Self {
        self.spacing = spacing;
        self
    }

    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn push(mut self, child: W) -> Self {
        self.children.push(child);
        self
    }
}

impl<W, M, T, R> Widget<M, T, R> for Column<W>
where
    W: Widget<M, T, R>,
{
    fn draw(&self, theme: &T) -> String {
        let children: Vec<String> = self
            .children
            .iter()
            .map(|child| child.draw(theme))
            .collect();
        format!(
            "column {:?} by {} [{}]",
            self.width,
            self.spacing,
            children.join(", ")
        )
    }

    fn press(&self) -> Option<M> {
        self.children.iter().find_map(Widget::press)
    }
}

/// Children over one another, as the iced fork's `Stack<W>`.
pub struct Stack<W> {
    children: Vec<W>,
}

impl<W> Stack<W> {
    pub fn with_children(children: impl IntoIterator<Item = W>) -> Self {
        Self {
            children: children.into_iter().collect(),
        }
    }
}

impl<W, M, T, R> Widget<M, T, R> for Stack<W>
where
    W: Widget<M, T, R>,
{
    fn draw(&self, theme: &T) -> String {
        let children: Vec<String> = self
            .children
            .iter()
            .map(|child| child.draw(theme))
            .collect();
        format!("stack [{}]", children.join(" over "))
    }

    fn press(&self) -> Option<M> {
        self.children.iter().rev().find_map(Widget::press)
    }
}

#[macro_export]
macro_rules! column {
    ($($child:expr),+ $(,)?) => (
        $crate::Column::with_children([$($crate::Widget::boxed($child)),+])
    );
}

#[macro_export]
macro_rules! stack {
    ($($child:expr),+ $(,)?) => (
        $crate::Stack::with_children([$($crate::Widget::boxed($child)),+])
    );
}

/// A layer that takes every press, as the iced fork's `opaque`.
pub fn opaque<'a, M, T, R>(content: impl Widget<M, T, R> + 'a) -> impl Widget<M, T, R> + 'a
where
    M: 'a,
    T: 'a,
    R: 'a,
{
    Opaque {
        content,
        input: PhantomData,
    }
}

struct Opaque<W, A> {
    content: W,
    input: PhantomData<A>,
}

impl<W, M, T, R> Widget<M, T, R> for Opaque<W, fn() -> (M, T, R)>
where
    W: Widget<M, T, R>,
{
    fn draw(&self, theme: &T) -> String {
        format!("opaque {}", self.content.draw(theme))
    }

    fn press(&self) -> Option<M> {
        self.content.press()
    }
}

/// Empty space, as `iced::widget::Space`.
pub struct Space {
    height: Length,
}

pub fn vertical_space() -> Space {
    Space {
        height: Length::Shrink,
    }
}

impl Space {
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }
}

impl<M, T, R> Widget<M, T, R> for Space {
    fn draw(&self, _theme: &T) -> String {
        format!("space {:?}", self.height)
    }
}

/// One child in a box of the theme's style, as the iced fork's
/// `Container<'a, W, Theme>`.
pub struct Container<'a, W, T = Theme> {
    content: W,
    width: Length,
    height: Length,
    center_x: bool,
    style: Box<dyn Fn(&T) -> u32 + 'a>,
}

pub fn container<'a, W, T>(content: W) -> Container<'a, W, T>
where
    T: Catalog + 'a,
{
    Container {
        content,
        width: Length::Shrink,
        height: Length::Shrink,
        center_x: false,
        style: Box::new(|_| 0),
    }
}

impl<'a, W, T> Container<'a, W, T> {
    pub fn width(mut self, width: impl Into<Length>) -> Self {
        self.width = width.into();
        self
    }

    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }

    pub fn center_x(mut self, width: impl Into<Length>) -> Self {
        self.center_x = true;
        self.width(width)
    }

    pub fn style(mut self, style: impl Fn(&T) -> u32 + 'a) -> Self {
        self.style = Box::new(style);
        self
    }
}

impl<W, M, T, R> Widget<M, T, R> for Container<'_, W, T>
where
    W: Widget<M, T, R>,
{
    fn draw(&self, theme: &T) -> String {
        let centered = if self.center_x { " centered" } else { "" };
        format!(
            "container {:?}x{:?}{centered} styled {} ({})",
            self.width,
            self.height,
            (self.style)(theme),
            self.content.draw(theme)
        )
    }

    fn press(&self) -> Option<M> {
        self.content.press()
    }
}

/// Content that scrolls, as the iced fork's `Scrollable<'a, Message, W,
/// Theme>`.
pub struct Scrollable<'a, M, W, T = Theme> {
    content: W,
    height: Length,
    input: PhantomData<(&'a T, fn() -> M)>,
}

pub fn scrollable<'a, M, W, T>(content: W) -> Scrollable<'a, M, W, T>
where
    T: Catalog + 'a,
{
    Scrollable {
        content,
        height: Length::Shrink,
        input: PhantomData,
    }
}

impl<M, W, T> Scrollable<'_, M, W, T> {
    pub fn height(mut self, height: impl Into<Length>) -> Self {
        self.height = height.into();
        self
    }
}

impl<M, W, T, R> Widget<M, T, R> for Scrollable<'_, M, W, T>
where
    W: Widget<M, T, R>,
{
    fn draw(&self, theme: &T) -> String {
        format!(
            "scrollable {:?} ({})",
            self.height,
            self.content.draw(theme)
        )
    }

    fn press(&self) -> Option<M> {
        self.content.press()
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

    /// The label as a button giving `message`, as an iced `Button`.
    pub fn on_press<M: Clone + 'a>(self, message: M) -> Element<'a, M, T>
    where
        T: 'a,
    {
        Pressable {
            label: self,
            message,
        }
        .boxed()
    }
}

impl<M, T, R> Widget<M, T, R> for Label<'_, T> {
    fn draw(&self, theme: &T) -> String {
        Label::draw(self, theme)
    }
}

/// A label that gives a message when pressed, as an iced `Button`.
struct Pressable<'a, M, T> {
    label: Label<'a, T>,
    message: M,
}

impl<M: Clone, T, R> Widget<M, T, R> for Pressable<'_, M, T> {
    fn draw(&self, theme: &T) -> String {
        self.label.draw(theme)
    }

    fn press(&self) -> Option<M> {
        Some(self.message.clone())
    }
}

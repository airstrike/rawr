//! iced's custom widgets: a program's own type implements the crate's
//! widget trait over any renderer its bound admits, as `impl<Message, Theme,
//! Renderer> Widget<Message, Theme, Renderer> for Circle where Renderer:
//! renderer::Renderer` does, and draws with the renderer's primitives.

use std::marker::PhantomData;

/// Invokes a provided trait method through the defining crate's path.
#[macro_export]
macro_rules! framed {
    ($widget:expr) => {
        $crate::widgets::Framed::framed($widget)
    };
}

/// A renderer's primitives, as iced's `renderer::Renderer`.
pub trait Renderer {
    /// Fills `quad` with `color`.
    fn fill_quad(&mut self, quad: Quad, color: crate::Color);
}

/// A quad a renderer fills, as iced's `renderer::Quad`.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Quad {
    /// Its size.
    pub size: u32,
    /// Its corners' radius.
    pub radius: u32,
}

/// The renderer custom widgets draw with: it records what it fills.
#[derive(Debug, Default)]
pub struct Recorder {
    filled: Vec<String>,
}

impl Renderer for Recorder {
    fn fill_quad(&mut self, quad: Quad, color: crate::Color) {
        self.filled.push(format!(
            "quad {}r{} {}",
            quad.size,
            quad.radius,
            crate::paint(color)
        ));
    }
}

mod backend {
    pub struct Primary;
    pub struct Secondary;
}

/// A renderer whose concrete backend types are not publicly nameable.
pub struct Composite<A, B> {
    recorder: Recorder,
    backends: PhantomData<(A, B)>,
}

/// The public name of one concrete renderer instance.
pub type Aliased = Composite<backend::Primary, backend::Secondary>;

/// Makes the renderer named only through `Aliased`.
pub fn aliased() -> Aliased {
    Composite {
        recorder: Recorder::default(),
        backends: PhantomData,
    }
}

/// A generic struct with a numeric default, like iced's `Size`.
#[derive(Debug, Clone, Copy)]
pub struct Size<T = f32> {
    /// Its width.
    pub width: T,
    /// Its height.
    pub height: T,
}

impl<T> Size<T> {
    /// Makes a size from its dimensions.
    pub fn new(width: T, height: T) -> Self {
        Self { width, height }
    }
}

/// Layout requests, like iced's `Length`.
#[derive(Debug, Clone, Copy)]
pub enum Length {
    /// Use the available space.
    Fill,
    /// Use the content's size.
    Shrink,
}

/// A native value lent mutably into a script callback.
pub struct Tree {
    /// Its computed size.
    pub size: Size,
}

/// Calls the script with a mutable native place and reports the stored size.
pub fn layout(update: impl Layout) -> String {
    let mut tree = Tree {
        size: Size::new(0.0, 0.0),
    };
    update.change(&mut tree);
    format!("{}x{}", tree.size.width, tree.size.height)
}

/// The layout operation implemented by a script's widget.
pub trait Layout {
    /// Updates the native layout tree in place.
    fn change(&self, tree: &mut Tree);
}

/// Describes a size instantiated at a type different from its default.
pub fn requested(size: Size<Length>) -> String {
    format!("{:?}/{:?}", size.width, size.height)
}

/// A second concrete instance used beside the first one.
pub fn numbers(size: Size<u16>) -> u16 {
    size.width + size.height
}

/// A constructor with a nested parameter and no default.
pub struct Nested<T> {
    /// The value that determines the parameter.
    pub value: Option<T>,
}

/// Reads the nested argument inferred by its constructor.
pub fn nested(value: Nested<u8>) -> u8 {
    value.value.unwrap_or(0)
}

impl<A, B> Renderer for Composite<A, B> {
    fn fill_quad(&mut self, quad: Quad, color: crate::Color) {
        self.recorder.fill_quad(quad, color);
    }
}

impl<A, B> Composite<A, B> {
    /// Returns what the renderer drew.
    pub fn rendered(&self) -> String {
        self.recorder.filled.join(", ")
    }
}

/// A widget a program writes itself, as iced's `advanced::Widget`, at a
/// theme and renderer that default to the crate's.
pub trait Widget<Message, Theme = crate::Theme, Renderer = Recorder> {
    /// Its size.
    fn size(&self) -> u32;

    /// Draws it with `renderer` at `theme`.
    fn draw(&self, renderer: &mut Renderer, theme: &Theme);

    /// The message a press on the button labelled `label` sends.
    fn press(&self, _label: &str) -> Option<Message> {
        None
    }
}

/// `widget` among the crate's own widgets, as iced's `column!` boxes a
/// custom widget.
pub fn custom<W, Message>(widget: W) -> Custom<W, Message>
where
    W: Widget<Message>,
{
    Custom {
        widget,
        look: PhantomData,
    }
}

/// A program's own widget among the crate's (`custom`).
pub struct Custom<W, Message> {
    widget: W,
    look: PhantomData<Message>,
}

impl<W, Message> crate::widgets::Widget<Message> for Custom<W, Message>
where
    W: Widget<Message>,
{
    fn labels(&self, labels: &mut Vec<String>) {
        let mut renderer = Recorder::default();
        self.widget.draw(&mut renderer, &crate::Theme);
        labels.push(format!(
            "custom {} [{}]",
            self.widget.size(),
            renderer.filled.join(", ")
        ));
    }

    fn press(&self, label: &str) -> Option<Message> {
        self.widget.press(label)
    }
}

/// A part of a view a program writes itself, as iced's `Component<'a,
/// Message>`, whose `view` gives `impl Widget<Self::Event> + 'a`.
pub trait Panel<'a, Message> {
    /// The panel's widgets.
    fn view(&self) -> impl crate::widgets::Widget<Message> + 'a;

    /// The message the panel sends for a press its widgets don't answer.
    fn fallback(&self, label: &str) -> Option<Message>;
}

/// `panel`'s widgets among the crate's own, as iced's `component(..)`.
pub fn panel<'a, P, Message>(panel: P) -> Paneled<'a, P, Message>
where
    P: Panel<'a, Message> + 'a,
    Message: 'a,
{
    Paneled {
        panel,
        look: PhantomData,
    }
}

/// A program's own panel among the crate's widgets (`panel`).
pub struct Paneled<'a, P, Message> {
    panel: P,
    look: PhantomData<&'a Message>,
}

impl<'a, P, Message> crate::widgets::Widget<Message> for Paneled<'a, P, Message>
where
    P: Panel<'a, Message>,
{
    fn labels(&self, labels: &mut Vec<String>) {
        labels.push("panel".to_owned());
        self.panel.view().labels(labels);
    }

    fn press(&self, label: &str) -> Option<Message> {
        self.panel
            .view()
            .press(label)
            .or_else(|| self.panel.fallback(label))
    }
}

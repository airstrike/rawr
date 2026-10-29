//! iced's custom widgets: a program's own type implements the crate's
//! widget trait over any renderer its bound admits, as `impl<Message, Theme,
//! Renderer> Widget<Message, Theme, Renderer> for Circle where Renderer:
//! renderer::Renderer` does, and draws with the renderer's primitives.

use std::marker::PhantomData;

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

/// A part of a view a program writes itself, as iced's `Component`, whose
/// `view` gives `impl Widget<Self::Event>`.
pub trait Panel<Message> {
    /// The panel's widgets.
    fn view(&self) -> impl crate::widgets::Widget<Message>;

    /// The message the panel sends for a press its widgets don't answer.
    fn fallback(&self, label: &str) -> Option<Message>;
}

/// `panel`'s widgets among the crate's own, as iced's `component(..)`.
pub fn panel<P, Message>(panel: P) -> Paneled<P, Message>
where
    P: Panel<Message>,
{
    Paneled {
        panel,
        look: PhantomData,
    }
}

/// A program's own panel among the crate's widgets (`panel`).
pub struct Paneled<P, Message> {
    panel: P,
    look: PhantomData<Message>,
}

impl<P, Message> crate::widgets::Widget<Message> for Paneled<P, Message>
where
    P: Panel<Message>,
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

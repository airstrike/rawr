//! Built-in controls that report themselves to an `Operation`.
//!
//! iced's toggler, radio and slider, and a checkbox without a label, call
//! nothing on an `Operation`, so a walk over the interface can't find
//! them. [`Reported`] wraps one and adds a single `Operation::custom` call
//! with its bounds and a [`Report`]: the control's own label and its kind.
//! Every other method goes to the control as it is, with the control's own
//! tree tag and state, so its widget tree, layout, drawing and events are
//! iced's. Operations that don't know a [`Report`] skip it, as they skip
//! any custom state they don't know. A text input reports itself too,
//! labelled by its placeholder, since iced reports it with its value.

use iced::advanced::widget::{tree, Meta, Tree};
use iced::advanced::{layout, overlay, renderer, Layout, Shell, Widget};
use iced::widget::Widget as _;
use iced::{mouse, Element, Length, Size, Vector};
use iced_runtime::core::widget::Operation;
use iced_runtime::core::{Event, Rectangle};

use crate::runtime;

/// What a control is, as a click or a key reaches it.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum Kind {
    /// A click anywhere on it toggles or selects it: a toggler, a radio,
    /// a checkbox.
    Toggle,
    /// A slider: with the cursor over it, ↓ moves it one step down and ↑
    /// one step up, as iced's slider reads its keys.
    Slider,
    /// A text input, labelled by its placeholder: a click focuses it and
    /// keys type into it. iced's text input reports itself to an
    /// `Operation` with its value, which a label must not show (a secure
    /// input's), so it reports itself here too.
    Input {
        /// Whether it hides what is typed, as a password's does.
        secure: bool,
    },
}

/// What a [`Reported`] control tells an `Operation` about itself.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Report {
    /// The control's own label; empty when it has none.
    pub(crate) label: String,
    pub(crate) kind: Kind,
}

/// A control that reports itself; see the module's documentation.
pub(crate) struct Reported {
    content: Element<'static, runtime::Message>,
    report: Report,
}

/// `content` reporting itself with `label` as a control of `kind`.
pub(crate) fn reported(
    content: Element<'static, runtime::Message>,
    label: Option<&str>,
    kind: Kind,
) -> Element<'static, runtime::Message> {
    Reported {
        content,
        report: Report {
            label: label.unwrap_or_default().to_owned(),
            kind,
        },
    }
    .boxed()
}

impl Meta for Reported {}

impl Widget<runtime::Message, iced::Theme, iced::Renderer> for Reported {
    fn tag(&self) -> tree::Tag {
        self.content.tag()
    }

    fn state(&self) -> tree::State {
        self.content.state()
    }

    fn diff(&mut self, tree: &mut Tree) {
        self.content.diff(tree);
    }

    fn size(&self) -> Size<Length> {
        self.content.size()
    }

    fn layout(&mut self, tree: &mut Tree, renderer: &iced::Renderer, limits: &layout::Limits) {
        self.content.layout(tree, renderer, limits);
    }

    fn operate(
        &mut self,
        tree: &mut Tree,
        layout: Layout,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
        operation: &mut dyn Operation,
    ) {
        self.content
            .operate(tree, layout, viewport, renderer, operation);
        operation.custom(None, layout.bounds(), &mut self.report);
    }

    fn update(
        &mut self,
        tree: &mut Tree,
        event: &Event,
        layout: Layout,
        cursor: mouse::Cursor,
        renderer: &iced::Renderer,
        shell: &mut Shell<'_, runtime::Message>,
        viewport: &Rectangle,
    ) {
        self.content
            .update(tree, event, layout, cursor, renderer, shell, viewport);
    }

    fn mouse_interaction(
        &self,
        tree: &Tree,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
        renderer: &iced::Renderer,
    ) -> mouse::Interaction {
        self.content
            .mouse_interaction(tree, layout, cursor, viewport, renderer)
    }

    fn draw(
        &self,
        tree: &Tree,
        renderer: &mut iced::Renderer,
        theme: &iced::Theme,
        style: &renderer::Style,
        layout: Layout,
        cursor: mouse::Cursor,
        viewport: &Rectangle,
    ) {
        self.content
            .draw(tree, renderer, theme, style, layout, cursor, viewport);
    }

    fn overlay<'b>(
        &'b mut self,
        tree: &'b mut Tree,
        layout: Layout,
        renderer: &iced::Renderer,
        viewport: &Rectangle,
        translation: Vector,
        window: Size,
    ) -> Vec<overlay::Element<'b, runtime::Message, iced::Theme, iced::Renderer>> {
        self.content
            .overlay(tree, layout, renderer, viewport, translation, window)
    }
}

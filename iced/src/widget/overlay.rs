//! Renderer-independent overlay protocol values for scripted widgets.

use rawr::host;

use crate::widget;

/// Validated overlay inputs supplied only by the iced adapter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Input {
    viewport: iced::Rectangle,
    translation: iced::Vector,
}

/// One sealed optional overlay descriptor.
///
/// The facade's logical `Message` parameter is compiler-owned. It is not a
/// Rust generic retained by this carrier.
#[derive(Clone)]
pub struct Value {
    witness: host::runtime::Witness,
    content: Option<widget::Value>,
}

impl Input {
    pub(crate) const fn new(viewport: iced::Rectangle, translation: iced::Vector) -> Self {
        Self {
            viewport,
            translation,
        }
    }

    pub(crate) const fn viewport(self) -> iced::Rectangle {
        self.viewport
    }

    pub(crate) const fn translation(self) -> iced::Vector {
        self.translation
    }
}

impl Value {
    pub(crate) const fn new(
        witness: host::runtime::Witness,
        content: Option<widget::Value>,
    ) -> Self {
        Self { witness, content }
    }

    pub(crate) const fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }

    pub(crate) const fn content(&self) -> Option<&widget::Value> {
        self.content.as_ref()
    }
}

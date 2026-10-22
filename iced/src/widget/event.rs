//! Renderer-independent scripted event protocol values.

use rawr::{host, Dynamic};

/// Validated event inputs supplied only by the iced adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Input {
    press: bool,
}

/// One sealed event status and optional application message.
///
/// The facade's logical `Message` parameter is compiler-owned. It is not a
/// Rust generic retained by this carrier.
#[derive(Clone)]
pub struct Outcome {
    witness: host::runtime::Witness,
    status: iced::event::Status,
    message: Option<Dynamic>,
}

impl Input {
    pub(crate) const fn new(press: bool) -> Self {
        Self { press }
    }

    pub(crate) const fn is_press(self) -> bool {
        self.press
    }
}

impl Outcome {
    pub(crate) const fn new(
        witness: host::runtime::Witness,
        status: iced::event::Status,
        message: Option<Dynamic>,
    ) -> Self {
        Self {
            witness,
            status,
            message,
        }
    }

    pub(crate) const fn witness(&self) -> &host::runtime::Witness {
        &self.witness
    }

    pub(crate) const fn status(&self) -> iced::event::Status {
        self.status
    }

    pub(crate) const fn message(&self) -> Option<&Dynamic> {
        self.message.as_ref()
    }

    pub(crate) fn when(mut self, enabled: bool) -> Self {
        if !enabled {
            self.status = iced::event::Status::Ignored;
            self.message = None;
        }
        self
    }
}

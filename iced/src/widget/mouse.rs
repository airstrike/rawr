//! Renderer-independent mouse protocol values for scripted widgets.

/// Validated pointer inputs supplied only by the iced adapter.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Input {
    over: bool,
}

impl Input {
    pub(crate) const fn new(over: bool) -> Self {
        Self { over }
    }

    pub(crate) const fn is_over(self) -> bool {
        self.over
    }
}

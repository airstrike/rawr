//! Renderer-independent operation protocol values for scripted widgets.

/// One bounded operation request supplied only by the iced adapter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Input {
    bounds: iced::Rectangle,
}

/// One sealed operation result.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Outcome {
    container: bool,
    traverse: bool,
}

impl Input {
    pub(crate) const fn new(bounds: iced::Rectangle) -> Self {
        Self { bounds }
    }

    pub(crate) const fn bounds(self) -> iced::Rectangle {
        self.bounds
    }
}

impl Outcome {
    pub(crate) const SKIP: Self = Self {
        container: false,
        traverse: false,
    };

    pub(crate) const CONTAINER: Self = Self {
        container: true,
        traverse: false,
    };

    pub(crate) const TRAVERSE: Self = Self {
        container: false,
        traverse: true,
    };

    pub(crate) const fn is_container(self) -> bool {
        self.container
    }

    pub(crate) const fn should_traverse(self) -> bool {
        self.traverse
    }
}

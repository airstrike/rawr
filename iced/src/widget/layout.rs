//! Renderer-independent scripted layout protocol values.

/// Validated layout inputs supplied only by the iced adapter.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Input {
    limits: iced::advanced::layout::Limits,
}

/// One validated owned layout result.
#[derive(Clone, Debug, PartialEq)]
pub struct Node {
    size: iced::Size,
    children: Vec<Child>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Child {
    index: usize,
    bounds: iced::Rectangle,
}

impl Input {
    pub(crate) const fn new(limits: iced::advanced::layout::Limits) -> Self {
        Self { limits }
    }

    pub(crate) fn resolve(
        self,
        requested: iced::Size<iced::Length>,
        intrinsic: iced::Size,
    ) -> Node {
        Node {
            size: self
                .limits
                .resolve(requested.width, requested.height, intrinsic),
            children: Vec::new(),
        }
    }
}

impl Node {
    pub(crate) fn child(mut self, index: usize, bounds: iced::Rectangle) -> Self {
        self.children.push(Child { index, bounds });
        self
    }

    pub(crate) const fn size(&self) -> iced::Size {
        self.size
    }

    pub(crate) fn children(&self) -> &[Child] {
        &self.children
    }
}

impl Child {
    pub(crate) const fn index(self) -> usize {
        self.index
    }

    pub(crate) const fn bounds(self) -> iced::Rectangle {
        self.bounds
    }
}

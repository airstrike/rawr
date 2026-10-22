//! Supported scripted-widget lifecycle hooks.

pub(crate) mod change;

use crate::{item, widget};

/// One supported renderer-independent lifecycle hook.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum Kind {
    /// Mint the initial iced tree-local state.
    State,
    /// Return the widget's direct children.
    Children,
    /// Reconcile retained local state with the new widget value.
    Diff,
    /// Declare the widget's requested length on both axes.
    Size,
    /// Transform validated limits into a layout node.
    Layout,
    /// Transform one event into an optional application message.
    Update,
    /// Produce owned validated drawing primitives.
    Draw,
    /// Select the current mouse interaction.
    MouseInteraction,
    /// Respond to one bounded widget operation.
    Operate,
    /// Produce an optional overlay value.
    Overlay,
}

/// The deterministic behavior used when a hook is omitted.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum Default {
    /// Use unit and allocate no iced tree-local state.
    UnitState,
    /// Retain no direct children.
    EmptyChildren,
    /// Perform no mutation or structural work.
    Noop,
    /// Request shrink length on both axes.
    Shrink,
    /// Produce a zero-size node with no children.
    ZeroLayout,
    /// Leave the event unclaimed and publish no message.
    IgnoredEvent,
    /// Perform no renderer operations.
    NoDraw,
    /// Report no mouse interaction.
    NoMouseInteraction,
    /// Produce no overlay.
    NoOverlay,
}

/// One exact symbolic hook selected by the compiler.
#[derive(Clone, Debug)]
pub struct Definition {
    kind: Kind,
    item: item::Value,
    location: widget::Location,
    output: crate::host::runtime::type_ref::Type,
}

impl Kind {
    /// Return the canonical source method name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::State => "state",
            Self::Children => "children",
            Self::Diff => "diff",
            Self::Size => "size",
            Self::Layout => "layout",
            Self::Update => "update",
            Self::Draw => "draw",
            Self::MouseInteraction => "mouse_interaction",
            Self::Operate => "operate",
            Self::Overlay => "overlay",
        }
    }

    /// Return the exact inert behavior used when this hook is omitted.
    #[must_use]
    pub const fn default_behavior(self) -> Default {
        match self {
            Self::State => Default::UnitState,
            Self::Children => Default::EmptyChildren,
            Self::Diff | Self::Operate => Default::Noop,
            Self::Size => Default::Shrink,
            Self::Layout => Default::ZeroLayout,
            Self::Update => Default::IgnoredEvent,
            Self::Draw => Default::NoDraw,
            Self::MouseInteraction => Default::NoMouseInteraction,
            Self::Overlay => Default::NoOverlay,
        }
    }

    pub(crate) fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "state" => Self::State,
            "children" => Self::Children,
            "diff" => Self::Diff,
            "size" => Self::Size,
            "layout" => Self::Layout,
            "update" => Self::Update,
            "draw" => Self::Draw,
            "mouse_interaction" => Self::MouseInteraction,
            "operate" => Self::Operate,
            "overlay" => Self::Overlay,
            _ => return None,
        })
    }
}

impl Definition {
    pub(crate) const fn new(
        kind: Kind,
        item: item::Value,
        location: widget::Location,
        output: crate::host::runtime::type_ref::Type,
    ) -> Self {
        Self {
            kind,
            item,
            location,
            output,
        }
    }

    /// Return the lifecycle role of this hook.
    #[must_use]
    pub const fn kind(&self) -> Kind {
        self.kind
    }

    /// Return the persistent symbolic method proof.
    #[must_use]
    pub const fn item(&self) -> &item::Value {
        &self.item
    }

    /// Return the exact hook declaration location.
    #[must_use]
    pub const fn location(&self) -> widget::Location {
        self.location
    }

    pub(crate) const fn output(&self) -> &crate::host::runtime::type_ref::Type {
        &self.output
    }
}

//! Durable symbolic message mapping for ordinary component values.

use iced::widget::Widget as _;
use rawr::item;

use crate::{runtime, widget};

pub(crate) struct Mapped {
    child: widget::Value,
    mapper: item::Value,
    identity: widget::Identity,
}

impl Mapped {
    pub(crate) fn new(child: widget::Value, mapper: item::Value) -> Self {
        Self {
            child,
            mapper,
            identity: widget::Identity::unique(),
        }
    }
}

impl widget::Descriptor for Mapped {
    fn lower(&self, context: &widget::Context<'_>) -> iced::Element<'static, runtime::Message> {
        let authority = context.mapping_authority();
        let child = context.child_mapping();
        let mapper = self.mapper.clone();
        self.child
            .lower(&child)
            .map(move |message| authority.map_message(message, &mapper))
            .boxed()
    }

    fn identity(&self) -> widget::Identity {
        self.identity.clone()
    }
}

//! Compiler-private transport for one stateful mutating widget hook.

use crate::{host, item, program, widget, Dynamic};
use std::fmt;

/// One sealed declared output paired with its final exact Tree-local state.
#[derive(Clone)]
pub(crate) struct Change {
    generation: program::GenerationId,
    item: item::Value,
    kind: widget::hook::Kind,
    state_type: host::runtime::type_ref::Type,
    output_type: host::runtime::type_ref::Type,
    state: Dynamic,
    output: Dynamic,
}

/// Values admitted by exact transport metadata.
pub(crate) struct Values {
    pub(crate) state: Dynamic,
    pub(crate) output: Dynamic,
}

/// A private change did not belong to the current exact hook invocation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum Error {
    Carrier { actual: &'static str },
    Generation,
    Item,
    Kind,
    StateType,
    OutputType,
    MissingState,
}

impl Change {
    /// Seal values already validated by the compiler-created support operation.
    pub(crate) fn seal(
        generation: program::GenerationId,
        item: item::Value,
        kind: widget::hook::Kind,
        state_type: host::runtime::type_ref::Type,
        output_type: host::runtime::type_ref::Type,
        state: Dynamic,
        output: Dynamic,
    ) -> Self {
        Self {
            generation,
            item,
            kind,
            state_type,
            output_type,
            state,
            output,
        }
    }

    /// Consume a transport only when every immutable invocation proof matches.
    pub(crate) fn open(
        self,
        generation: program::GenerationId,
        item: &item::Value,
        kind: widget::hook::Kind,
        state_type: &host::runtime::type_ref::Type,
        output_type: &host::runtime::type_ref::Type,
    ) -> Result<Values, Error> {
        if self.generation != generation {
            return Err(Error::Generation);
        }
        if self.item != *item {
            return Err(Error::Item);
        }
        if self.kind != kind {
            return Err(Error::Kind);
        }
        if self.state_type != *state_type {
            return Err(Error::StateType);
        }
        if self.output_type != *output_type {
            return Err(Error::OutputType);
        }
        Ok(Values {
            state: self.state,
            output: self.output,
        })
    }
}

impl Error {
    pub(crate) fn carrier(value: &Dynamic) -> Self {
        Self::Carrier {
            actual: value.type_name(),
        }
    }
}

impl fmt::Debug for Change {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Change(<private>)")
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Carrier { actual } => {
                write!(
                    formatter,
                    "expected private widget hook change, found `{actual}`"
                )
            }
            Self::Generation => formatter.write_str("widget hook change generation mismatch"),
            Self::Item => formatter.write_str("widget hook change item mismatch"),
            Self::Kind => formatter.write_str("widget hook change kind mismatch"),
            Self::StateType => formatter.write_str("widget hook change State type mismatch"),
            Self::OutputType => formatter.write_str("widget hook change output type mismatch"),
            Self::MissingState => {
                formatter.write_str("stateful mutating widget hook is missing Tree-local State")
            }
        }
    }
}

impl std::error::Error for Error {}

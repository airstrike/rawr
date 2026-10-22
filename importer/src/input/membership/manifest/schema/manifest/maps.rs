//! Complete finite enclosing collections from Cargo's original manifest.

mod features;
mod patch;
mod platforms;
mod replace;
mod table;
mod targets;

#[cfg(test)]
mod tests;

use crate::input::membership::manifest::{Span, Stop};

pub(in crate::input::membership::manifest) use features::Features;
pub(in crate::input::membership::manifest) use patch::Patch;
pub(in crate::input::membership::manifest) use platforms::Platforms;
pub(in crate::input::membership::manifest) use replace::Replace;
pub(in crate::input::membership::manifest) use table::Entry;
pub(in crate::input::membership::manifest) use targets::Targets;

fn locate(stop: Stop, span: Option<Span>) -> Stop {
    match stop {
        Stop::Rejected(mut diagnostic) => {
            diagnostic.span = diagnostic.span.or(span);
            Stop::Rejected(diagnostic)
        }
        Stop::Refused => Stop::Refused,
    }
}

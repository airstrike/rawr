//! Complete pinned profiles with flat recursive ownership and leaf visitors.

mod debug;
mod opt;
pub(in super::super) mod tree;

#[cfg(test)]
mod tests;

use rawr_toml::document;

use super::super::{session, value, Diagnostic, Step, Tag};

pub(in super::super) use debug::Debug;
pub(in super::super) use opt::Opt;
pub(in super::super) use tree::{Location, Profiles};

/// Serde's u32 visitor accepts signed/unsigned 64-bit dispatch only when it fits.
pub(in super::super) fn units<A: document::Admission>(
    input: value::Input<'_, '_>,
    session: &mut session::Session<'_, A>,
) -> Step<u32> {
    let converted = match input.view(session)? {
        value::View::Scalar(value::Kind::I64(value)) => u32::try_from(value),
        value::View::Scalar(value::Kind::U64(value)) => u32::try_from(value),
        _ => {
            return session.reject(Diagnostic {
                tag: Tag::Type,
                span: input.span,
                expected: Some("u32"),
            });
        }
    };
    match converted {
        Ok(value) => Ok(value),
        Err(_) => session.reject(Diagnostic {
            tag: Tag::Variant,
            span: input.span,
            expected: Some("u32"),
        }),
    }
}

// These two untagged visitors install only an i64 callback. The pinned
// dispatcher tries checked conversions into installed callbacks, including
// for u64/i128/u128 events; callback rejection does not resume dispatch.
fn signed(kind: value::Kind<'_>) -> Option<i64> {
    match kind {
        value::Kind::I64(value) => Some(value),
        value::Kind::U64(value) => i64::try_from(value).ok(),
        value::Kind::I128(value) => i64::try_from(value).ok(),
        value::Kind::U128(value) => i64::try_from(value).ok(),
        _ => None,
    }
}

//! Cargo's boolean, checked-i64 and exact-string debug-information visitor.

use rawr_toml::document;

use super::super::super::{session, value, Diagnostic, Step, Tag};

const EXPECTED: &str =
    "a boolean, 0, 1, 2, none, limited, full, line-tables-only, or line-directives-only";

/// The pinned enum's distinct values and declared comparison order.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub(in super::super::super) enum Debug {
    None,
    LineDirectivesOnly,
    LineTablesOnly,
    Limited,
    Full,
}

impl Debug {
    pub(in super::super::super) fn interpret<A: document::Admission>(
        input: value::Input<'_, '_>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        let view = input.view(session)?;
        let (converted, tag) = match view {
            value::View::Scalar(value::Kind::Boolean(value)) => {
                return Ok(if value { Self::Full } else { Self::None });
            }
            value::View::Scalar(value::Kind::String(text)) => (
                match text.as_str() {
                    "none" => Some(Self::None),
                    "limited" => Some(Self::Limited),
                    "full" => Some(Self::Full),
                    "line-directives-only" => Some(Self::LineDirectivesOnly),
                    "line-tables-only" => Some(Self::LineTablesOnly),
                    _ => None,
                },
                Tag::Variant,
            ),
            value::View::Scalar(kind) => match super::signed(kind) {
                Some(value) => (
                    match value {
                        0 => Some(Self::None),
                        1 => Some(Self::Limited),
                        2 => Some(Self::Full),
                        _ => None,
                    },
                    Tag::Variant,
                ),
                None => (None, Tag::Type),
            },
            _ => (None, Tag::Type),
        };
        match converted {
            Some(value) => Ok(value),
            None => session.reject(Diagnostic {
                tag,
                span: input.span,
                expected: Some(EXPECTED),
            }),
        }
    }
}

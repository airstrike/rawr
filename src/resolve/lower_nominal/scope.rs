//! The parser's scopes: the bindings a body makes, the frames that hold
//! them, and the loops and branches they are read in.

use super::ty::Ty;
use super::{iterator, origin, pattern, ParsedExpr};
use crate::resolve::borrowck;
use crate::{ImmutableString, Position, Span};
use std::collections::BTreeSet;

/// A `for` loop's iterator when it holds borrows, its element, the
/// positions where the element takes what the iterator holds (pairs of the
/// element's position and the iterator's), and its pattern's bindings. The
/// element and the bindings are given a value each iteration.
pub(super) type LoopElement = (
    Option<crate::resolve::borrowck::Local>,
    crate::resolve::borrowck::Local,
    Vec<(borrowck::Path, borrowck::Path)>,
    Vec<crate::resolve::borrowck::Local>,
);

#[derive(Clone)]
pub(super) struct Binding {
    pub(super) ty: Ty,
    pub(super) mutable: bool,
    pub(super) available: bool,
    pub(super) declaration: Span,
    pub(super) kind: BindingKind,
}

#[derive(Clone)]
pub(super) enum BindingKind {
    Local,
    Invocation,
    Receiver { owner: ImmutableString },
}

/// An array literal being parsed, for the frames its elements open: a
/// call that opens one runs ahead of the statement, so each element before
/// its own is bound ahead of that frame, as Rust evaluates elements in
/// order (`[icon().size(12), Slot::from("Verified")]`).
pub(super) struct FrameArray {
    /// Where the literal starts.
    pub(super) start: Position,
    /// How many statements' frames were open when it started: a call
    /// opening a frame of the same statement binds its elements.
    pub(super) depth: usize,
    /// How many calls had been noted when it started (`calls_noted`),
    /// counted rather than placed, as a macro's tokens all sit at its
    /// call.
    pub(super) noted: usize,
    /// How many calls had been noted when the element being parsed
    /// started, its index, and how many frames its statement had then.
    pub(super) element: (usize, usize, usize),
    /// Each element whose frame needs the elements before it bound first,
    /// with how many frames its statement had when it started.
    pub(super) bound: Vec<(usize, usize)>,
}

/// A crate struct literal's named fields, each with where its name is,
/// and its `..base`, each value parsed in source order.
#[cfg(feature = "rust-import")]
pub(super) struct RecordFields {
    pub(super) named: Vec<(ImmutableString, Span, ParsedExpr)>,
    pub(super) base: Option<ParsedExpr>,
}

/// An array literal's elements, its array's repeat count and a `vec!`'s
/// count expression (`[x; 3]`, `vec![x; n]`).
pub(super) type ArrayElements = (Vec<ParsedExpr>, Option<usize>, Option<ParsedExpr>);

/// A scope frame a statement's call opens (D558 tier 1): `outer` declares
/// what keeps the frame, `inner` binds and opens it ahead of the statement,
/// and `after` stores the copies it gives back once it has ended.
#[derive(Default)]
pub(super) struct Frame {
    pub(super) outer: origin::Fragment,
    pub(super) inner: origin::Fragment,
    pub(super) after: origin::Fragment,
}

pub(super) struct Branch {
    pub(super) value: ParsedExpr,
    pub(super) syntax: Span,
}

/// An iterator chain bound to an immutable local: the local's declaration,
/// the chain, its item type, and the names its setup bound where the local
/// was bound, which a closure using the local captures.
#[derive(Clone)]
pub(super) struct BoundIterator {
    pub(super) declaration: Span,
    pub(super) pipeline: iterator::Pipeline,
    pub(super) item: Ty,
    pub(super) names: Vec<String>,
}

/// A loop being parsed. A `break` or `continue` to an enclosing loop sets
/// that loop's jump flag and breaks out of every loop in between.
pub(super) struct LoopFrame {
    pub(super) label: Option<ImmutableString>,
    pub(super) jump: String,
    /// `loop`, which may `break` with a value.
    pub(super) value: bool,
    pub(super) ty: Option<Ty>,
    /// A nested loop jumps to this one.
    pub(super) targeted: bool,
    /// Enclosing loops (by depth) that a jump inside this loop targets.
    pub(super) exits: BTreeSet<usize>,
    /// The depth of `drop_scopes` outside the loop's body: a jump to the
    /// loop drops the scopes above it.
    pub(super) drop_depth: usize,
    /// The loop's depth in the recorded control flow, which also counts a
    /// `while` whose condition holds this loop.
    pub(super) flow_depth: usize,
    /// While a `while`'s condition is read: whether a labeled jump in it
    /// goes to this `while`.
    pub(super) condition: Option<bool>,
}

/// A `let`'s local and its type (`Lowering::local_types`).
pub(super) struct LocalType {
    pub(super) name: ImmutableString,
    pub(super) declaration: Span,
    pub(super) ty: Ty,
    /// The `let`'s value, when it has one.
    pub(super) value: Option<Span>,
}

/// A use of a `let x;` before an assignment gives it a value
/// (`Lowering::uninitialized`).
pub(super) struct UninitializedUse {
    pub(super) name: ImmutableString,
    pub(super) declaration: Span,
    pub(super) reference: Span,
    pub(super) ty: Ty,
    /// Whether some path to the use gives the local a value.
    pub(super) possibly: bool,
    /// The operator the use is the operand of while its type is unknown,
    /// which needs the type there: rustc's code for it and its place.
    pub(super) operator: Option<(&'static str, Span)>,
}

/// Whether `pattern` binds some part of its value by value: a name, not a
/// `ref`, a `_` or a `&` pattern.
pub(super) fn binds_by_value(pattern: &pattern::Syntax) -> bool {
    match pattern {
        pattern::Syntax::Binding { .. } => true,
        pattern::Syntax::Rest { binding, .. } => binding.is_some(),
        pattern::Syntax::TupleStruct { members, .. } | pattern::Syntax::Tuple { members, .. } => {
            members.iter().any(binds_by_value)
        }
        pattern::Syntax::Or { alternatives, .. } => alternatives.iter().any(binds_by_value),
        pattern::Syntax::Prelude { payload, .. } => payload.as_deref().is_some_and(binds_by_value),
        pattern::Syntax::Cow { payload, .. } => binds_by_value(payload),
        pattern::Syntax::Slice {
            before,
            rest,
            after,
            ..
        } => {
            before.iter().chain(after.iter()).any(binds_by_value)
                || rest.as_deref().is_some_and(binds_by_value)
        }
        _ => false,
    }
}

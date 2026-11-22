//! A block's tail temporaries that outlive the block's locals, as they do
//! before edition 2024, and rustc's explanation of a borrow of a local one
//! of them keeps past the local's end (E0597).

use super::borrowck::{Body, Local};
use crate::{ImmutableString, Span};

/// What dropping a temporary runs, as rustc names it.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Destructor {
    /// The `Drop` impl of the type at this path: `Guard`.
    Drop(ImmutableString),
    /// The drop glue of this type, which has no `Drop` impl of its own:
    /// `Option<Guard<'_>>`.
    Glue(ImmutableString),
}

impl Destructor {
    /// What the drop runs: `the `Drop` code for type `Guard``.
    fn runs(&self) -> String {
        match self {
            Self::Drop(path) => format!("the `Drop` code for type `{path}`"),
            Self::Glue(ty) => format!("the destructor for type `{ty}`"),
        }
    }
}

/// A temporary of a block's tail whose drop runs after the locals of the
/// block, and the tail it is part of.
#[derive(Clone, Debug)]
pub(crate) struct Outliving {
    local: Local,
    tail: Span,
    destructor: Destructor,
    /// Where it drops, when that is the end of a statement around its
    /// block, not its body's.
    dropped: Option<Span>,
}

impl Outliving {
    /// The temporary `local` of the tail spanning `tail`, whose drop runs
    /// `destructor`.
    pub(crate) const fn new(local: Local, tail: Span, destructor: Destructor) -> Self {
        Self {
            local,
            tail,
            destructor,
            dropped: None,
        }
    }

    pub(crate) const fn local(&self) -> Local {
        self.local
    }

    /// It drops at `at`, where the statement around its block ends.
    pub(crate) fn drop_at(&mut self, at: Span) {
        self.dropped = Some(at);
    }
}

/// rustc's E0597 for a local whose borrow a tail temporary's drop uses
/// after the local ends.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Explanation {
    message: ImmutableString,
    name: ImmutableString,
    declared: Span,
    borrow: Span,
    temporary: Span,
    /// Where the local ends.
    dropped: Span,
    /// Where the temporary drops.
    used: Span,
    tail: Span,
    destructor: Destructor,
}

/// rustc's note on a tail temporary that outlives its block's locals.
const NOTE: &str = "the temporary is part of an expression at the end of a block;\n\
                    consider forcing this temporary to be dropped sooner, before the block's \
                    local variables are dropped";

/// The name rustc's help gives the local it suggests.
const SUGGESTED: &str = "x";

impl Explanation {
    /// Why the loan `loan` of `local`, which ends at `dropped`, is live
    /// there, when a tail temporary that outlives the local holds it.
    /// `reach` is `borrowck::region::reach` of `body`.
    pub(crate) fn of(
        body: &Body,
        reach: &[Vec<bool>],
        loan: usize,
        local: Local,
        dropped: Span,
    ) -> Option<Self> {
        let reached = reach.get(loan)?;
        let outliving = body
            .outliving
            .iter()
            .find(|outliving| reached[outliving.local.0 as usize])?;
        let info = &body.locals[local.0 as usize];
        Some(Self {
            message: format!("`{}` does not live long enough", info.name).into(),
            name: info.name.clone(),
            declared: info.declaration,
            borrow: body.loans[loan].span,
            temporary: body.locals[outliving.local.0 as usize].declaration,
            dropped,
            used: outliving.dropped.unwrap_or(dropped),
            tail: outliving.tail,
            destructor: outliving.destructor.clone(),
        })
    }

    /// rustc's message.
    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    /// The borrow, which rustc reports at.
    pub(crate) const fn borrow(&self) -> Span {
        self.borrow
    }

    /// The explanation with the destructor of a temporary that is std's
    /// guard of a cell's borrow named by `named`, from the temporary's
    /// span, once the body's types are settled (`Ref<'_, i32>`).
    pub(crate) fn guard_named(mut self, named: impl Fn(Span) -> Option<ImmutableString>) -> Self {
        if let Some(guard) = named(self.temporary) {
            self.destructor = Destructor::Glue(guard);
        }
        self
    }

    /// The explanation with each expression's span from where its first
    /// and last tokens start, as the facts give them, to where it is
    /// `written` in the source.
    pub(crate) fn written(mut self, written: impl Fn(Span) -> Span) -> Self {
        self.borrow = written(self.borrow);
        self.temporary = written(self.temporary);
        self.tail = written(self.tail);
        self
    }

    pub(crate) const fn label() -> &'static str {
        "borrowed value does not live long enough"
    }

    /// rustc's other labels, each at its place, in the order it lists
    /// those at one place.
    pub(crate) fn labels(&self) -> [(Span, String); 4] {
        let name = &self.name;
        [
            (self.declared, format!("binding `{name}` declared here")),
            (
                self.temporary,
                "a temporary with access to the borrow is created here ...".to_owned(),
            ),
            (
                self.dropped,
                format!("`{name}` dropped here while still borrowed"),
            ),
            (
                self.used,
                format!(
                    "... and the borrow might be used here, when that temporary is dropped and \
                     runs {}",
                    self.destructor.runs()
                ),
            ),
        ]
    }

    pub(crate) const fn note() -> &'static str {
        NOTE
    }

    /// rustc's help, and its edits: a `let` of the tail before it and the
    /// new local after it.
    pub(crate) fn help(&self) -> (String, [(Span, String); 2]) {
        let start = Span::new(self.tail.start(), self.tail.start());
        let end = Span::new(self.tail.end(), self.tail.end());
        (
            format!(
                "for example, you could save the expression's value in a new local variable \
                 `{SUGGESTED}` and then make `{SUGGESTED}` be the expression at the end of the \
                 block"
            ),
            [
                (start, format!("let {SUGGESTED} = ")),
                (end, format!("; {SUGGESTED}")),
            ],
        )
    }
}

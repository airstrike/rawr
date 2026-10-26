//! Rawr's borrow checker (D572). `lower_nominal` records a [`Body`] for each
//! function body as it lowers it: its locals, the places it reads, writes,
//! moves and borrows, the loans those borrows make, where each loan flows,
//! and the body's control flow. [`check`] solves it after the body, with a
//! [`Rule`] that says where each loan is live; NLL's is [`nll::Nll`].
//!
//! The facts are the rule's input and nothing else, so another rule, such
//! as the Polonius alpha stable rustc is expected to adopt, takes the same
//! facts (D572.11).
//!
//! What the facts do not model yet is not checked here: such a loan runs,
//! and the background `cargo check` decides (D572.5). A loan this module
//! does model is never left to rustc.

pub(crate) mod nll;
pub(crate) mod record;
pub(crate) mod signature;

#[cfg(test)]
mod differential;

use crate::{ImmutableString, Span};

/// A local of the body: a `let`, a pattern's binding, a parameter, or a
/// hidden one a borrow's value is held in until something takes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Local(pub(crate) u32);

/// A loan: a borrow's place, its kind and where it was made.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct LoanId(pub(crate) u32);

/// A block of the body's control flow.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Block(pub(crate) u32);

/// A point: before the fact at `index` of `block`, or before the block's
/// end when `index` is its length.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Point {
    pub(crate) block: Block,
    pub(crate) index: u32,
}

/// One step below a local in a place.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Projection {
    /// A named field or a tuple's member, by its text (`a`, `0`).
    Field(ImmutableString),
    /// An element (`v[i]`): every element overlaps every other, as rustc
    /// has it for borrows.
    Index,
    /// Through a reference.
    Deref,
}

/// A local and the steps below it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Place {
    pub(crate) local: Local,
    pub(crate) projection: Vec<Projection>,
}

/// What an access does to its place.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Access {
    /// A copy or a use that reads the place.
    Read,
    /// A shared borrow, as `&place` or an autoref'd `&self` call.
    Shared,
    /// A two-phase `&mut` borrow's reservation (`v.push(v.len())`): it
    /// conflicts as a shared borrow does until its activation.
    Reserve,
    /// A `&mut` borrow, or a two-phase one's activation.
    Mutable,
    /// An assignment to the place, which writes it without reading.
    Write,
    /// A move out of the place.
    Move,
    /// The local's storage ends (its block ends) with a loan of it live.
    StorageDead,
    /// Nothing: an access the lowering took back (`v = x` reads no `v`).
    None,
}

/// Where a loan or a holder's loans flow: into a local's type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Origin {
    Loan(LoanId),
    Local(Local),
}

/// One fact of a block, in the order the body runs them.
#[derive(Clone, Debug)]
pub(crate) enum Fact {
    /// A use of the local: it is live before this point.
    Use(Local),
    /// The local is given a new value (`let`, an assignment): it is dead
    /// before this point unless something uses it here.
    Def(Local),
    /// An access to a place, which may make a loan.
    Access {
        place: Place,
        access: Access,
        span: Span,
        loan: Option<LoanId>,
    },
    /// Nothing: a fact the lowering took back.
    Nop,
}

/// A loan's place and kind, and where the body made it.
#[derive(Clone, Debug)]
pub(crate) struct Loan {
    pub(crate) place: Place,
    pub(crate) mutable: bool,
    /// A two-phase `&mut` borrow's reservation (`v.push(v.len())`): until
    /// its activation it lets the place be read and borrowed shared.
    pub(crate) two_phase: bool,
    pub(crate) span: Span,
    pub(crate) at: Point,
    /// The hidden local the borrow's value is held in until a `let`, an
    /// assignment or a call takes it: the loan's point defines it.
    pub(crate) temporary: Local,
}

/// A local's name and declaration, for messages.
#[derive(Clone, Debug)]
pub(crate) struct LocalInfo {
    pub(crate) name: ImmutableString,
    pub(crate) declaration: Span,
}

/// A block's facts and the blocks control flows to after it.
#[derive(Clone, Debug, Default)]
pub(crate) struct BlockData {
    pub(crate) facts: Vec<Fact>,
    pub(crate) successors: Vec<Block>,
}

/// A loan flowing into a local at a point: the local's region is within
/// the loan's (`'loan: 'local`).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Flow {
    pub(crate) from: Origin,
    pub(crate) into: Local,
    /// Where it flows. NLL's constraints hold at every point and do not
    /// read it; a location-sensitive rule such as the Polonius alpha does.
    #[allow(dead_code)]
    pub(crate) at: Point,
}

/// The facts of one function body.
#[derive(Clone, Debug, Default)]
pub(crate) struct Body {
    pub(crate) blocks: Vec<BlockData>,
    pub(crate) locals: Vec<LocalInfo>,
    pub(crate) loans: Vec<Loan>,
    pub(crate) flows: Vec<Flow>,
    /// What this body does that the facts do not model yet, with where:
    /// loans these touch are left to rustc.
    pub(crate) unmodeled: Vec<(&'static str, Span)>,
    /// Locals whose region is every point: a loan flowing into one is
    /// borrowed for `'static`.
    pub(crate) everywhere: Vec<Local>,
}

impl Body {
    /// The points after `point`.
    pub(crate) fn successors(&self, point: Point) -> impl Iterator<Item = Point> + '_ {
        let block = &self.blocks[point.block.0 as usize];
        let next = (point.index < block.facts.len() as u32).then_some(Point {
            block: point.block,
            index: point.index + 1,
        });
        let jumps = if next.is_some() {
            &[][..]
        } else {
            &block.successors[..]
        };
        next.into_iter()
            .chain(jumps.iter().map(|&block| Point { block, index: 0 }))
    }

    /// The fact at `point`, if it is not a block's end.
    pub(crate) fn fact(&self, point: Point) -> Option<&Fact> {
        self.blocks[point.block.0 as usize]
            .facts
            .get(point.index as usize)
    }

    /// Every point of the body, block by block.
    pub(crate) fn points(&self) -> impl Iterator<Item = Point> + '_ {
        self.blocks.iter().enumerate().flat_map(|(block, data)| {
            (0..=data.facts.len() as u32).map(move |index| Point {
                block: Block(block as u32),
                index,
            })
        })
    }
}

/// The rule that says where each loan is live, from the body's facts.
pub(crate) trait Rule {
    /// For each loan, the points where it is in scope, by [`point_index`]:
    /// an access there that conflicts with it is an error.
    fn loans_in_scope(&self, body: &Body) -> Vec<Points>;
}

/// A set of a body's points, by [`point_index`].
#[derive(Clone, Debug)]
pub(crate) struct Points(Vec<u64>);

impl Points {
    pub(crate) fn new(count: usize) -> Self {
        Self(vec![0; count.div_ceil(64)])
    }

    /// Adds `point`; whether it was not there.
    pub(crate) fn insert(&mut self, point: usize) -> bool {
        let (word, bit) = (point / 64, 1 << (point % 64));
        let new = self.0[word] & bit == 0;
        self.0[word] |= bit;
        new
    }

    pub(crate) fn contains(&self, point: usize) -> bool {
        self.0[point / 64] & (1 << (point % 64)) != 0
    }

    pub(crate) fn union(&mut self, other: &Self) {
        for (word, other) in self.0.iter_mut().zip(&other.0) {
            *word |= other;
        }
    }
}

/// A borrow error, as rustc reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Conflict {
    /// rustc's code: E0499, E0502, E0503, E0505, E0506 or E0597.
    pub(crate) code: &'static str,
    pub(crate) message: String,
    /// The access that conflicts.
    pub(crate) access: Span,
    /// The loan it conflicts with.
    pub(crate) borrow: Span,
}

/// The first borrow error in `body` under `rule`, in the order the body
/// runs, as rustc reports the first of a place's conflicts at one access.
pub(crate) fn check(body: &Body, rule: &impl Rule) -> Option<Conflict> {
    if body.loans.is_empty() {
        return None;
    }
    let in_scope = rule.loans_in_scope(body);
    let index = point_index(body);
    // Only a loan of the same local can conflict with an access.
    let mut by_local = std::collections::BTreeMap::<Local, Vec<usize>>::new();
    for (loan_index, loan) in body.loans.iter().enumerate() {
        by_local
            .entry(loan.place.local)
            .or_default()
            .push(loan_index);
    }
    let forever = forever(body);
    for point in body.points() {
        let Some(Fact::Access {
            place,
            access,
            span,
            loan,
        }) = body.fact(point)
        else {
            continue;
        };
        // A local borrowed for `'static` never lives long enough: it ends
        // with the body, or when it unwinds.
        if let Some(loan) = loan
            && forever[loan.0 as usize]
            && !body.loans[loan.0 as usize]
                .place
                .projection
                .contains(&Projection::Deref)
        {
            let loan = &body.loans[loan.0 as usize];
            return Some(Conflict {
                code: "E0597",
                message: format!(
                    "`{}` does not live long enough",
                    describe(body, &loan.place)
                ),
                access: loan.span,
                borrow: loan.span,
            });
        }
        let Some(loans) = by_local.get(&place.local) else {
            continue;
        };
        // A loan is in scope at its own point only from an earlier
        // iteration of a loop, where it conflicts with itself.
        for &loan_index in loans {
            if !in_scope[loan_index].contains(index(point)) {
                continue;
            }
            if let Some(conflict) = conflict(body, place, *access, *span, &body.loans[loan_index]) {
                return Some(conflict);
            }
        }
    }
    None
}

/// Whether each loan flows into a local of [`Body::everywhere`]: whether it
/// is borrowed for `'static`.
fn forever(body: &Body) -> Vec<bool> {
    let mut reaches = vec![false; body.locals.len()];
    for local in &body.everywhere {
        reaches[local.0 as usize] = true;
    }
    // Backwards over the flows, until nothing changes.
    let mut changed = !body.everywhere.is_empty();
    while changed {
        changed = false;
        for flow in &body.flows {
            if let Origin::Local(from) = flow.from
                && reaches[flow.into.0 as usize]
                && !reaches[from.0 as usize]
            {
                reaches[from.0 as usize] = true;
                changed = true;
            }
        }
    }
    body.loans
        .iter()
        .enumerate()
        .map(|(index, loan)| {
            reaches[loan.temporary.0 as usize]
                || body.flows.iter().any(|flow| {
                    flow.from == Origin::Loan(LoanId(index as u32)) && reaches[flow.into.0 as usize]
                })
        })
        .collect()
}

/// The index of each point in a flat numbering of the body's points.
pub(crate) fn point_index(body: &Body) -> impl Fn(Point) -> usize + '_ {
    let mut starts = Vec::with_capacity(body.blocks.len());
    let mut total = 0;
    for block in &body.blocks {
        starts.push(total);
        total += block.facts.len() + 1;
    }
    move |point: Point| starts[point.block.0 as usize] + point.index as usize
}

/// The number of points in the body.
pub(crate) fn point_count(body: &Body) -> usize {
    body.blocks.iter().map(|block| block.facts.len() + 1).sum()
}

/// How two places overlap: not at all, or with what is left of the loan's
/// place below the accessed one.
fn overlap<'a>(accessed: &Place, borrowed: &'a Place) -> Option<&'a [Projection]> {
    if accessed.local != borrowed.local {
        return None;
    }
    for (index, (a, b)) in accessed
        .projection
        .iter()
        .zip(&borrowed.projection)
        .enumerate()
    {
        match (a, b) {
            (Projection::Field(a), Projection::Field(b)) if a != b => return None,
            (Projection::Field(_), Projection::Field(_))
            | (Projection::Index, Projection::Index)
            | (Projection::Deref, Projection::Deref) => {}
            // Different kinds of step below one place: rustc treats them
            // as overlapping, as a field of an indexed element does.
            _ => return Some(&borrowed.projection[index..]),
        }
    }
    let shared = accessed.projection.len().min(borrowed.projection.len());
    Some(&borrowed.projection[shared..])
}

fn conflict(
    body: &Body,
    place: &Place,
    access: Access,
    span: Span,
    loan: &Loan,
) -> Option<Conflict> {
    let below = overlap(place, &loan.place)?;
    // A write or a storage end replaces the place itself: what a loan
    // reached through a reference stored there stays valid.
    let shallow = matches!(access, Access::Write | Access::StorageDead);
    if shallow && below.contains(&Projection::Deref) {
        return None;
    }
    let name = describe(body, place);
    // A reservation conflicts as a shared borrow does, and is reported as
    // the `&mut` borrow it becomes.
    let (conflicts_mutably, reported_mutable) = (loan.mutable, loan.mutable || loan.two_phase);
    let (code, message) = match access {
        Access::None => return None,
        Access::Reserve | Access::Mutable if !conflicts_mutably && reported_mutable => (
            "E0499",
            format!("cannot borrow `{name}` as mutable more than once at a time"),
        ),
        Access::Read if loan.mutable => (
            "E0503",
            format!("cannot use `{name}` because it was mutably borrowed"),
        ),
        Access::Shared if loan.mutable => (
            "E0502",
            format!("cannot borrow `{name}` as immutable because it is also borrowed as mutable"),
        ),
        Access::Read | Access::Shared | Access::Reserve if !loan.mutable => return None,
        Access::Reserve | Access::Mutable if loan.mutable => (
            "E0499",
            format!("cannot borrow `{name}` as mutable more than once at a time"),
        ),
        Access::Mutable => (
            "E0502",
            format!("cannot borrow `{name}` as mutable because it is also borrowed as immutable"),
        ),
        Access::Write => (
            "E0506",
            format!("cannot assign to `{name}` because it is borrowed"),
        ),
        Access::Move => (
            "E0505",
            format!("cannot move out of `{name}` because it is borrowed"),
        ),
        // rustc reports this one at the borrow, which the end outlives.
        Access::StorageDead => {
            return Some(Conflict {
                code: "E0597",
                message: format!("`{name}` does not live long enough"),
                access: loan.span,
                borrow: span,
            });
        }
        Access::Read | Access::Shared | Access::Reserve => unreachable!("matched above"),
    };
    Some(Conflict {
        code,
        message,
        access: span,
        borrow: loan.span,
    })
}

/// A place as rustc names it: `v`, `s.a`, `*r`, `r.a` through `r`, `v[_]`.
pub(crate) fn describe(body: &Body, place: &Place) -> String {
    let mut text = body.locals[place.local.0 as usize].name.to_string();
    for (index, step) in place.projection.iter().enumerate() {
        match step {
            Projection::Field(field) => {
                text.push('.');
                text.push_str(field);
            }
            Projection::Index => text.push_str("[_]"),
            Projection::Deref if place.projection.len() == index + 1 => {
                text = format!("*{text}");
            }
            Projection::Deref => {}
        }
    }
    text
}

#[cfg(test)]
thread_local! {
    /// What the bodies checked on this thread did that the facts do not
    /// model yet, for the differential harness to tell its misses apart.
    pub(crate) static UNMODELED: std::cell::RefCell<Vec<(&'static str, Span)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(test)]
pub(crate) fn note_body(body: &Body) {
    UNMODELED.with(|notes| notes.borrow_mut().extend(body.unmodeled.iter().copied()));
}

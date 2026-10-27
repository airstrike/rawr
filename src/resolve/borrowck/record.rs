//! How `lower_nominal` records a body's facts as it reads the body: one
//! [`Recorder`] per body, told about each use of a local, each borrow,
//! assignment and move, and each branch, loop and jump, in source order.
//!
//! A use of a local is recorded where it is read, as a read of the local;
//! what the lowering then makes of it (a field of it, a borrow of it, an
//! assignment to it, a `&mut self` call on it) refines that record, found
//! by where the use starts.

use super::region::{self, Held, Path, Step};
use super::{
    Access, Block, BlockData, Body, Capture, Fact, Flow, Loan, LoanId, Local, LocalInfo, Origin,
    Place, Point, Projection,
};
use crate::{ImmutableString, Position, Span};
use std::collections::{BTreeMap, BTreeSet};

/// A loop being read: where `continue` goes and the blocks that leave it.
struct LoopFrame {
    header: Block,
    exits: Vec<Block>,
}

/// A call's argument: where it is, whether its type may hold a borrow, and
/// whether calling it may return a borrow.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Argument {
    pub(crate) span: Span,
    pub(crate) holds: bool,
    pub(crate) returns: bool,
}

/// What a value expression began with, so the `let` that takes it can tell
/// what it made: [`Recorder::value_start`].
#[derive(Clone, Copy)]
pub(crate) struct ValueStart {
    loans: usize,
    borrowed: usize,
}

/// What a pattern matches against ([`Recorder::matched`]).
#[derive(Clone, Debug)]
pub(crate) enum Matched {
    /// A place: a pattern's parts are places below it.
    Place(Place),
    /// A reference made for the match (`&mut s`): what a pattern binds
    /// below it holds these loans, and what it reads through it conflicts
    /// with none.
    Origins(Held),
    /// A value of its own, not a reference, that holds these loans (a
    /// call's value, `v.first()`): what a pattern binds in it holds them.
    Held(Held),
    /// A value of its own, made for the match: whether making it borrowed,
    /// which a binding that may hold a borrow would keep.
    Value { made: bool },
}

/// A value an expression made and holds until its temporary scope ends
/// (`String::from("a")` of `String::from("a").as_str()`), or its block's
/// when a `let` extends it (`let r = &String::from("a");`).
#[derive(Clone, Copy, Debug)]
struct Temporary {
    local: Local,
    span: Span,
    /// Made by `&` in a place a `let` may extend: not a call's receiver.
    extendable: bool,
    /// Its value's drop uses the borrows it holds: a use where it ends.
    drops: bool,
    /// Where it ends unless a `let` extends it: the end of the innermost
    /// scope it outlived because that scope's value may borrow it (a
    /// block's tail), held by a [`Fact::Nop`] until then.
    ends: Option<Point>,
}

/// How a temporary scope ends ([`Recorder::close_temporaries`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ScopeEnd {
    /// A `let`: a temporary its value borrows in an extending place lives
    /// to the end of the `let`'s block.
    Let,
    /// A statement ending in `;`, or a condition: its temporaries end.
    Statement,
    /// A scope whose value goes on into the expression around it (a
    /// block's tail, a match arm): a temporary that value may borrow in an
    /// extending place goes on with it.
    Value,
}

/// The facts of a body being read.
pub(crate) struct Recorder {
    body: Body,
    current: Block,
    locals: BTreeMap<Span, Local>,
    /// The access each use of a local made, by where the use starts.
    mentions: BTreeMap<Position, Point>,
    /// The loans and the holders' loans each expression's value is, by its
    /// span, at the positions of its type: `&v`, `r`, `if c { &a } else { &b }`.
    origins: BTreeMap<Span, Held>,
    /// The holders used, the places a call borrowed, and the closures that
    /// captured, so far: the borrows a value may have been made from, for
    /// [`ValueStart`].
    borrowed: usize,
    loops: Vec<LoopFrame>,
    /// The loops around each closure being read, which its body cannot
    /// jump to.
    outer_loops: Vec<Vec<LoopFrame>>,
    /// For the body and each closure being read: the blocks that return.
    returns: Vec<Vec<Block>>,
    /// The uses that went through an overloaded index (`v[i]` of a `Vec`):
    /// `Index::index(&v)`, a borrow of all of `v`.
    overloaded: BTreeSet<Point>,
    /// The uses that are a match's subject, which patterns do not move.
    subjects: BTreeSet<Point>,
    /// The last call's operands, used once it is made.
    operands: Vec<Local>,
    /// The borrow each use through an overloaded index made, by the use.
    index_loans: BTreeMap<Point, LoanId>,
    /// The holders' uses read through an explicit `*`.
    dereferenced: BTreeSet<Point>,
    /// Expressions that are places although the lowering gave them no
    /// place of its own (`*r` of a shared `r`).
    places: BTreeSet<Span>,
    /// Where each two-phase `&mut` borrow for a call is activated, by the
    /// use it reserved.
    activations: BTreeMap<Point, Point>,
    /// Each call's arguments, by where its `)` is.
    arguments: BTreeMap<Position, Vec<Argument>>,
    /// The hidden local whose region is every point: what flows into it is
    /// borrowed for `'static`.
    everywhere: Option<Local>,
    /// The temporary scopes being read, innermost last, with the
    /// temporaries made in each.
    temporary_scopes: Vec<Vec<Temporary>>,
    /// The blocks being read, innermost last, with the temporaries a `let`
    /// in each extended to its end.
    blocks: Vec<Vec<Temporary>>,
    /// Temporaries a call took a borrow of, which no `let` extends.
    pinned: BTreeSet<Local>,
    /// A constant's initializer, whose value's temporaries are `'static`.
    constant: bool,
    /// Where each call without arguments ends.
    calls: BTreeSet<Position>,
    /// Where each tuple struct's or variant's constructor ends (`Some(1)`),
    /// which is no call that makes a value at run time.
    constructors: BTreeSet<Position>,
    /// The value each part (a field, an element) is read from, whether that
    /// value is a reference, and whether it is made at run time, by the
    /// part's span.
    projections: BTreeMap<Span, (Span, bool, bool)>,
    /// For each closure being read: what its `return`s give, or `None` for
    /// one these facts do not follow.
    closure_returns: Vec<Vec<Option<Held>>>,
    /// Each closure of the body, as [`Recorder::captures`] found it.
    closures: BTreeMap<crate::item::Key, Closure>,
    /// What a call of each closure keeps, by where the closure is written.
    closure_keeps: BTreeMap<Span, Option<Held>>,
}

/// A closure, as [`Recorder::captures`] finds it.
#[derive(Clone, Debug, Default)]
pub(crate) struct Closure {
    /// The borrows its value holds.
    pub(crate) origins: Held,
    /// What a call of it keeps, or `None` where these facts do not follow
    /// it.
    pub(crate) keeps: Option<Held>,
    /// Whether it changes what it captures (`FnMut`): a call borrows it
    /// `&mut`.
    pub(crate) mutable: bool,
    /// Whether it is `Copy`: it captures nothing by `&mut` and nothing by
    /// value that is not `Copy`, as rustc has it.
    pub(crate) copy: bool,
    /// The places it captures by value: each captured local's declaration,
    /// the path below it, and whether the place is copied rather than
    /// moved.
    pub(crate) by_value: Vec<(Span, Vec<Projection>, bool)>,
}

/// A local a closure captures, as the lowering knows it: where it is
/// declared, whether its type may hold borrows, whether it is `Copy`, and
/// whether it was declared `mut`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Captured {
    pub(crate) declaration: Span,
    pub(crate) holder: bool,
    pub(crate) copy: bool,
    pub(crate) declared_mut: bool,
}

/// A closure's body once read: its blocks.
pub(crate) struct ClosureBody {
    blocks: std::ops::Range<u32>,
    returns: Vec<Option<Held>>,
}

/// How a closure captures a place, from weakest to strongest, as rustc's
/// capture analysis orders them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum CaptureKind {
    Shared,
    Mutable,
    ByValue,
}

/// A place a closure captures and how.
struct CapturePlace {
    local: Local,
    projection: Vec<Projection>,
    kind: CaptureKind,
    /// For a capture by value: whether the place is copied rather than
    /// moved, when that is known.
    copy: Option<bool>,
}

impl Default for Recorder {
    fn default() -> Self {
        Self {
            body: Body {
                blocks: vec![BlockData::default()],
                ..Body::default()
            },
            current: Block(0),
            locals: BTreeMap::new(),
            mentions: BTreeMap::new(),
            origins: BTreeMap::new(),
            borrowed: 0,
            loops: Vec::new(),
            outer_loops: Vec::new(),
            returns: vec![Vec::new()],
            overloaded: BTreeSet::new(),
            subjects: BTreeSet::new(),
            dereferenced: BTreeSet::new(),
            operands: Vec::new(),
            index_loans: BTreeMap::new(),
            places: BTreeSet::new(),
            activations: BTreeMap::new(),
            arguments: BTreeMap::new(),
            everywhere: None,
            temporary_scopes: Vec::new(),
            blocks: Vec::new(),
            pinned: BTreeSet::new(),
            constant: false,
            calls: BTreeSet::new(),
            constructors: BTreeSet::new(),
            projections: BTreeMap::new(),
            closure_returns: Vec::new(),
            closures: BTreeMap::new(),
            closure_keeps: BTreeMap::new(),
        }
    }
}

impl Recorder {
    /// The facts recorded, once the body is read.
    pub(crate) fn finish(mut self) -> Body {
        self.flush();
        self.body
    }

    fn point(&self) -> Point {
        Point {
            block: self.current,
            index: self.body.blocks[self.current.0 as usize].facts.len() as u32,
        }
    }

    fn push(&mut self, fact: Fact) -> Point {
        self.flush();
        let point = self.point();
        self.body.blocks[self.current.0 as usize].facts.push(fact);
        point
    }

    /// The last call's operands are used where it is made: here, after a
    /// `&mut self` call's activation if it had one.
    fn flush(&mut self) {
        for local in std::mem::take(&mut self.operands) {
            self.body.blocks[self.current.0 as usize]
                .facts
                .push(Fact::Use(local));
        }
    }

    /// A call's arguments, spanning `arguments`, are its operands: the
    /// borrows they are live until the call is made. `end` is where its
    /// `)` is.
    pub(crate) fn call_operands(&mut self, arguments: &[Argument], end: Position) {
        self.flush();
        self.arguments.insert(end, arguments.to_vec());
        for argument in arguments {
            for (_, origin) in self
                .origins
                .get(&argument.span)
                .cloned()
                .unwrap_or_default()
            {
                let local = self.origin_local(origin);
                self.operands.push(local);
            }
        }
    }

    /// The local an origin's loans are in: a holder, or a borrow's hidden
    /// local.
    fn origin_local(&self, origin: Origin) -> Local {
        match origin {
            Origin::Loan(loan) => self.body.loans[loan.0 as usize].temporary,
            Origin::Local(local, _) => local,
        }
    }

    /// Where an origin's borrows flow from: a holder's position, or a
    /// borrow's hidden local.
    fn source(&self, origin: Origin) -> Origin {
        match origin {
            Origin::Loan(loan) => Origin::local(self.body.loans[loan.0 as usize].temporary),
            origin @ Origin::Local(..) => origin,
        }
    }

    /// `into` takes the loans of the value spanning `value` here, which
    /// are used here: whether it is a value these facts follow.
    fn take_origins(&mut self, into: Local, value: Span) -> bool {
        let Some(held) = self.origins.get(&value).cloned() else {
            return false;
        };
        self.flow_into(into, &held);
        true
    }

    /// `into` takes `held` here, each at its position, which are used here.
    fn flow_into(&mut self, into: Local, held: &[(Path, Origin)]) {
        for &(path, origin) in held {
            let from = self.source(origin);
            let at = self.push(Fact::Use(self.origin_local(origin)));
            self.body.flows.push(Flow {
                from,
                into,
                path,
                at,
            });
        }
    }

    /// The call whose `)` is at `end`: its arguments.
    pub(crate) fn arguments(&self, end: Position) -> Option<Vec<Argument>> {
        self.arguments.get(&end).cloned()
    }

    /// The loans and holders the value spanning `span` is, when these facts
    /// follow it.
    pub(crate) fn origins(&self, span: Span) -> Option<&[(Path, Origin)]> {
        self.origins.get(&span).map(Vec::as_slice)
    }

    /// The value spanning `span` is `held`: it holds those borrows and no
    /// others.
    pub(crate) fn set_origins(&mut self, span: Span, held: Held) {
        self.origins.insert(span, held);
    }

    /// The value spanning `whole` holds what the one spanning `part` does
    /// (`(r)`, `&v.first()`), when these facts follow that one.
    pub(crate) fn same_origins(&mut self, whole: Span, part: Span) {
        if whole != part
            && let Some(origins) = self.origins.get(&part).cloned()
        {
            self.origins.insert(whole, origins);
        }
    }

    /// The holder the use starting at `start` reads, when it reads all of
    /// a local's value: what that value holds.
    pub(crate) fn held(&self, start: Position) -> Option<Origin> {
        let (_, place) = self.mention_place(start)?;
        place
            .projection
            .is_empty()
            .then_some(Origin::local(place.local))
    }

    /// The use starting at `start`, a call's receiver, is borrowed for the
    /// call, and the call's value keeps that borrow (`v.first()`): the
    /// loan, made where the receiver is read, or for `&mut self` where the
    /// call activates its two-phase borrow. `inner` when what the receiver
    /// holds is kept too, as a reference keeps what it refers to.
    pub(crate) fn autoref(
        &mut self,
        start: Position,
        span: Span,
        mutable: bool,
        inner: bool,
    ) -> Option<Origin> {
        let (point, place) = self.mention_place(start)?;
        if let Some(loan) = self.index_loans.get(&point).copied() {
            return Some(Origin::Loan(loan));
        }
        let loan = if mutable {
            let activation = *self.activations.get(&point)?;
            match self.body.fact(activation) {
                Some(Fact::Access {
                    access: Access::Mutable,
                    loan: None,
                    ..
                }) => {}
                Some(Fact::Access {
                    loan: Some(loan), ..
                }) => return Some(Origin::Loan(*loan)),
                _ => return None,
            }
            self.make_loan(activation, place.clone(), true, span)
        } else {
            match self.body.fact(point) {
                Some(Fact::Access {
                    access: Access::Read | Access::Shared,
                    loan: None,
                    ..
                }) => {}
                Some(Fact::Access {
                    loan: Some(loan),
                    access: Access::Shared | Access::Mutable,
                    ..
                }) => return Some(Origin::Loan(*loan)),
                _ => return None,
            }
            let loan = self.make_loan(point, place.clone(), false, span);
            self.set_access(point, Access::Shared);
            // The receiver's borrow is the call's operand: it lasts over the
            // arguments to the call, made here.
            let temporary = self.body.loans[loan.0 as usize].temporary;
            self.push(Fact::Use(temporary));
            loan
        };
        // Through a holder (`r.first()` of `r: &Vec<i32>`) the loan
        // reborrows what `r` borrows; of a local holding borrows it keeps
        // them.
        if place.projection.first() == Some(&Projection::Deref) || inner {
            let temporary = self.body.loans[loan.0 as usize].temporary;
            self.body.flows.push(Flow {
                from: Origin::local(place.local),
                into: temporary,
                path: Path::ROOT,
                at: point,
            });
        }
        Some(Origin::Loan(loan))
    }

    /// The place the use starting at `start` reads takes `held` here, as
    /// `v.push(&x)` stores a borrow in `v`: whether it is a local's value
    /// below no reference, which these facts follow. Into all of a local,
    /// `held` is placed as it is; into a part of one, everywhere in it.
    pub(crate) fn store(&mut self, start: Position, held: &[(Path, Origin)]) -> bool {
        let Some((_, place)) = self.mention_place(start) else {
            return false;
        };
        if place.projection.contains(&Projection::Deref) {
            return false;
        }
        if place.projection.is_empty() {
            self.flow_into(place.local, held);
        } else {
            self.flow_into(place.local, &region::collapse(held));
        }
        true
    }

    /// A field of the local the use starting at `start` reads takes `held`
    /// here, already placed at the local's positions (`p.y = &c`); another
    /// place takes it as [`Recorder::store`] does.
    pub(crate) fn store_field(&mut self, start: Position, held: &[(Path, Origin)]) -> bool {
        let Some((_, place)) = self.mention_place(start) else {
            return false;
        };
        if !matches!(place.projection[..], [Projection::Field(_)]) {
            return self.store(start, held);
        }
        self.flow_into(place.local, held);
        true
    }

    /// The function returns `held`, its value spanning `span`, here: it
    /// flows into its output's region. A returned borrow of one of its own
    /// locals is rustc's E0515 ([`super::check`]). A borrow of a reference
    /// itself may be a deref coercion's reborrow of what it refers to,
    /// which these facts do not tell apart. These two notes read the flows
    /// through any position, more than the check finds.
    pub(crate) fn give_back(&mut self, held: &[(Path, Origin)], span: Span) {
        if self.constant {
            return;
        }
        let local = self.hidden("<return>", span);
        self.body.returned.push(local);
        self.flow_into(local, held);
        let returned = super::reaches(&self.body, &[local]);
        if self
            .body
            .loans
            .iter()
            .zip(returned)
            .any(|(loan, returned)| returned && loan.of_reference && loan.capture == Capture::No)
        {
            self.unmodeled("a borrow of a reference that a function returns", span);
        }
        // A parameter's borrows returned where the signature's lifetimes
        // are several, which these facts do not tell apart.
        if let Some(own) = &self.body.own {
            let reached = super::reaching(&self.body, &[local]);
            let several = own.parameters.iter().any(|(parameter, borrows, _)| {
                reached[parameter.0 as usize] && borrows.outlive(&own.output, &own.bounds).is_none()
            });
            if several {
                self.unmodeled(
                    "a returned borrow whose lifetimes these facts do not tell apart",
                    span,
                );
            }
        }
    }

    /// `held` is borrowed for `'static` here: its region is every point.
    pub(crate) fn forever(&mut self, held: &[(Path, Origin)]) {
        let local = match self.everywhere {
            Some(local) => local,
            None => {
                let local = self.hidden("<'static>", Span::NONE);
                self.body.everywhere.push(local);
                self.everywhere = Some(local);
                local
            }
        };
        self.flow_into(local, held);
    }

    fn fact_mut(&mut self, point: Point) -> &mut Fact {
        &mut self.body.blocks[point.block.0 as usize].facts[point.index as usize]
    }

    /// A new block; what the block control is in still owes is recorded in
    /// it first.
    fn new_block(&mut self) -> Block {
        self.flush();
        self.body.blocks.push(BlockData::default());
        Block(self.body.blocks.len() as u32 - 1)
    }

    fn edge(&mut self, from: Block, to: Block) {
        let successors = &mut self.body.blocks[from.0 as usize].successors;
        if !successors.contains(&to) {
            successors.push(to);
        }
    }

    /// The local declared at `declaration`.
    pub(crate) fn local(&mut self, name: &str, declaration: Span) -> Local {
        if let Some(local) = self.locals.get(&declaration) {
            return *local;
        }
        let local = self.hidden(name, declaration);
        self.locals.insert(declaration, local);
        local
    }

    fn hidden(&mut self, name: &str, declaration: Span) -> Local {
        self.body.locals.push(LocalInfo {
            name: name.into(),
            declaration,
        });
        Local(self.body.locals.len() as u32 - 1)
    }

    /// Something in the body these facts do not model yet: what it touches
    /// is left to rustc.
    pub(crate) fn unmodeled(&mut self, what: &'static str, span: Span) {
        self.body.unmodeled.push((what, span));
    }

    /// A use of the local `name` declared at `declaration`, starting at
    /// `span`. A holder (a local whose type may hold a borrow) is used, and
    /// the use's value is what it holds. Through a `reference`, the place
    /// read is what it refers to.
    pub(crate) fn mention(
        &mut self,
        name: &str,
        declaration: Span,
        holder: bool,
        reference: bool,
        span: Span,
    ) {
        let local = self.local(name, declaration);
        if holder || reference {
            self.push(Fact::Use(local));
            self.borrowed += 1;
            self.origins
                .insert(span, vec![(Path::ROOT, Origin::local(local))]);
        }
        let projection = if reference {
            vec![Projection::Deref]
        } else {
            Vec::new()
        };
        let point = self.push(Fact::Access {
            place: Place { local, projection },
            access: Access::Read,
            span,
            loan: None,
        });
        self.mentions.insert(span.start(), point);
    }

    fn mentioned(&self, start: Position) -> Option<Point> {
        self.mentions.get(&start).copied()
    }

    /// The use starting at `start` reads a step below what it read.
    pub(crate) fn project(&mut self, start: Position, step: Projection) {
        if let Some(point) = self.mentioned(start)
            && !self.overloaded.contains(&point)
            && let Fact::Access { place, .. } = self.fact_mut(point)
        {
            place.projection.push(step);
        }
    }

    /// The use starting at `start` is indexed through `Index` (`v[i]` of a
    /// `Vec`, a `String`, a map): it borrows all of what it used, and what
    /// is below the element is that borrow's.
    pub(crate) fn overloaded_index(&mut self, start: Position, span: Span) {
        let Some((point, place)) = self.mention_place(start) else {
            return;
        };
        if !self.overloaded.insert(point)
            || !matches!(
                self.body.fact(point),
                Some(Fact::Access {
                    access: Access::Read,
                    loan: None,
                    ..
                })
            )
        {
            return;
        }
        let loan = self.make_loan(point, place, false, span);
        self.set_access(point, Access::Shared);
        self.index_loans.insert(point, loan);
    }

    /// The index of the use starting at `start` is read: `Index::index`
    /// is called here, and its borrow lasts as long as what it gave.
    pub(crate) fn index_end(&mut self, start: Position) {
        if let Some(point) = self.mentioned(start)
            && let Some(loan) = self.index_loans.get(&point).copied()
        {
            let temporary = self.body.loans[loan.0 as usize].temporary;
            self.push(Fact::Use(temporary));
        }
    }

    /// What a pattern matches against: the value spanning `span`, begun at
    /// `start`, whose use starts at `begin` when `place` says it is a place.
    /// A place's use reads nothing itself: the pattern reads, moves and
    /// borrows the parts it tests and binds ([`Recorder::part`]).
    pub(crate) fn matched(
        &mut self,
        begin: Position,
        span: Span,
        place: bool,
        start: ValueStart,
    ) -> Matched {
        if place
            && let Some((point, mut place)) = self.mention_place(begin)
            && let Some(Fact::Access {
                access: Access::Read | Access::Shared | Access::Move,
                loan: None,
                ..
            }) = self.body.fact(point)
        {
            self.set_access(point, Access::None);
            self.subjects.insert(point);
            // A bare holder is matched as the reference it is.
            if place.projection == [Projection::Deref] && !self.dereferenced.contains(&point) {
                place.projection.clear();
            }
            return Matched::Place(place);
        }
        if let Some(origins) = self.origins.get(&span) {
            return Matched::Origins(origins.clone());
        }
        Matched::Value {
            made: self.body.loans.len() > start.loans || self.borrowed > start.borrowed,
        }
    }

    /// A pattern reads (`Read`, `Length`) or moves (`Move`) the part
    /// `place` of what it matches, at `span`.
    pub(crate) fn part(&mut self, place: Place, access: Access, span: Span) {
        if place.projection.first() == Some(&Projection::Deref) {
            self.push(Fact::Use(place.local));
        }
        self.push(Fact::Access {
            place,
            access,
            span,
            loan: None,
        });
    }

    /// A pattern binds `into` by reference to the part `place` of what it
    /// matches (`ref x`, or a binding below a reference): a borrow of the
    /// part made here, at `span`, which `into` takes, given its value here
    /// when `define` says so (once for an or-pattern's alternatives).
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn part_borrow(
        &mut self,
        place: Place,
        mutable: bool,
        holds: bool,
        span: Span,
        into: Local,
        define: bool,
    ) {
        // A part that holds borrows (`ref x` of a `&T` field) keeps them, as
        // `&'a T<'b>` has `'b: 'a`.
        let inner = (holds && !place.projection.contains(&Projection::Deref))
            .then(|| region::narrow(place_origin(&place), Path::OWN));
        let through = (place.projection.first() == Some(&Projection::Deref)).then_some(place.local);
        if let Some(holder) = through {
            self.push(Fact::Use(holder));
        }
        let point = self.push(Fact::Access {
            place: place.clone(),
            access: if mutable {
                Access::Mutable
            } else {
                Access::Shared
            },
            span,
            loan: None,
        });
        let loan = self.make_loan(point, place, mutable, span);
        let temporary = self.body.loans[loan.0 as usize].temporary;
        // A reborrow through a holder keeps what the holder borrows.
        for from in through.map(Origin::local).into_iter().chain(inner) {
            self.body.flows.push(Flow {
                from,
                into: temporary,
                path: Path::ROOT,
                at: point,
            });
        }
        self.take(into, &[(Path::ROOT, Origin::Loan(loan))], define);
    }

    /// `into` takes `held` here: a pattern's binding of a reference's part,
    /// or of the reference itself. It is given its value here when `define`
    /// says so.
    pub(crate) fn take(&mut self, into: Local, held: &[(Path, Origin)], define: bool) {
        if define {
            self.push(Fact::Def(into));
        }
        self.flow_into(into, held);
    }

    /// The expression spanning `span` is a place (`*r`).
    pub(crate) fn mark_place(&mut self, span: Span) {
        self.places.insert(span);
    }

    /// Whether the expression spanning `span` is a place [`mark_place`]
    /// marked.
    ///
    /// [`mark_place`]: Recorder::mark_place
    pub(crate) fn is_place(&self, span: Span) -> bool {
        self.places.contains(&span)
    }

    /// The use starting at `start` is also known by an expression starting
    /// at `alias` (`*r` for `r`), when a later step finds it from there.
    pub(crate) fn alias(&mut self, start: Position, alias: Position) {
        if let Some(point) = self.mentioned(start) {
            self.mentions.insert(alias, point);
        }
    }

    /// `*r` (starting at `alias`) of the use starting at `start`: a holder
    /// used through `*` is what it refers to even where a bare use would
    /// be the holder itself (`&mut *r`, against `&mut r`).
    pub(crate) fn deref(&mut self, start: Position, alias: Position) {
        if let Some(point) = self.mentioned(start) {
            self.mentions.insert(alias, point);
            self.dereferenced.insert(point);
        }
    }

    /// The place the use starting at `start` accesses, and the holder it
    /// goes through, if any.
    fn mention_place(&self, start: Position) -> Option<(Point, Place)> {
        let point = self.mentioned(start)?;
        match self.body.fact(point) {
            Some(Fact::Access { place, .. }) => Some((point, place.clone())),
            _ => None,
        }
    }

    fn set_access(&mut self, point: Point, to: Access) {
        if let Fact::Access { access, .. } = self.fact_mut(point) {
            *access = to;
        }
    }

    /// The use starting at `start` is a shared borrow for a call on it
    /// (`v.len()`) rather than a read.
    pub(crate) fn shared(&mut self, start: Position) {
        if let Some(point) = self.mentioned(start)
            && matches!(
                self.body.fact(point),
                Some(Fact::Access {
                    access: Access::Read,
                    ..
                })
            )
        {
            self.set_access(point, Access::Shared);
            self.borrowed += 1;
        }
    }

    /// The use starting at `start` is moved.
    pub(crate) fn moved(&mut self, start: Position) {
        if let Some(point) = self.mentioned(start)
            && !self.subjects.contains(&point)
        {
            self.set_access(point, Access::Move);
        }
    }

    /// `&place` or `&mut place` (spanning `span`) of the use starting at
    /// `start`, whose operand spans `operand`: the use becomes the borrow,
    /// which makes a loan. `inner` when the borrowed place's type may hold
    /// borrows, which the reference keeps (`&'a T<'b>` has `'b: 'a`) below
    /// its own region; `reference` when the place is a reference itself.
    pub(crate) fn borrow(
        &mut self,
        start: Position,
        operand: Span,
        span: Span,
        mutable: bool,
        inner: bool,
        reference: bool,
    ) -> Option<LoanId> {
        let (point, mut place) = self.mention_place(start)?;
        // `&r` and `&mut r` of a holder borrow the holder itself.
        let of_reference =
            place.projection == [Projection::Deref] && !self.dereferenced.contains(&point);
        if of_reference {
            place.projection.clear();
        }
        let of_reference = of_reference || reference;
        // `&v[i]` of a `Vec` is the borrow `Index::index` made.
        let loan = match self.index_loans.get(&point).copied() {
            Some(loan) => {
                self.body.loans[loan.0 as usize].mutable = mutable;
                loan
            }
            None => self.make_loan(point, place.clone(), mutable, span),
        };
        self.body.loans[loan.0 as usize].of_reference = of_reference;
        self.set_access(
            point,
            if mutable {
                Access::Mutable
            } else {
                Access::Shared
            },
        );
        let temporary = self.body.loans[loan.0 as usize].temporary;
        // What the place holds, below the reference's own region.
        let below = match self.origins.get(&operand) {
            Some(held) if inner => held.clone(),
            _ if inner => vec![(Path::ROOT, place_origin(&place))],
            _ => Vec::new(),
        };
        // A reborrow through a holder (`&mut *r`, `&r.a`) keeps what the
        // holder holds, and a borrow of a place that holds borrows (`&v` of
        // a `Vec<&i32>`) keeps them, for as long as it lives.
        let reborrowed = (place.projection.first() == Some(&Projection::Deref))
            .then(|| Origin::local(place.local));
        let kept: Vec<Origin> = region::collapse(&below)
            .into_iter()
            .map(|(_, origin)| self.source(origin))
            .collect();
        for from in reborrowed.into_iter().chain(kept) {
            self.body.flows.push(Flow {
                from,
                into: temporary,
                path: Path::ROOT,
                at: point,
            });
        }
        let mut held = vec![(Path::OWN, Origin::Loan(loan))];
        held.extend(region::place(&below, Path::of(&[Step::Arg(0)])));
        self.origins.insert(span, held);
        Some(loan)
    }

    fn make_loan(&mut self, point: Point, place: Place, mutable: bool, span: Span) -> LoanId {
        let temporary = self.hidden("<borrow>", span);
        self.body.loans.push(Loan {
            place,
            mutable,
            two_phase: false,
            capture: Capture::No,
            span,
            at: point,
            temporary,
            of_reference: false,
        });
        let loan = LoanId(self.body.loans.len() as u32 - 1);
        if let Fact::Access { loan: made, .. } = self.fact_mut(point) {
            *made = Some(loan);
        }
        loan
    }

    /// The use starting at `start` is the receiver of a `&mut self` call
    /// (or another `&mut` borrow for a call), reserved where it was read
    /// and activated here, after the arguments, as rustc's two-phase
    /// borrows are.
    pub(crate) fn mutable_call(&mut self, start: Position, span: Span) {
        let Some((point, place)) = self.mention_place(start) else {
            return;
        };
        // Through an overloaded index the borrow is `IndexMut`'s, made
        // where the receiver is read and kept to the call.
        if let Some(loan) = self.index_loans.get(&point).copied() {
            if self.body.loans[loan.0 as usize].mutable {
                return;
            }
            self.body.loans[loan.0 as usize].mutable = true;
            self.set_access(point, Access::Mutable);
            let temporary = self.body.loans[loan.0 as usize].temporary;
            self.push(Fact::Use(temporary));
            self.flush();
            return;
        }
        // An explicit `&mut place` is its own borrow already.
        if !matches!(
            self.body.fact(point),
            Some(Fact::Access {
                access: Access::Read | Access::Shared,
                loan: None,
                ..
            })
        ) {
            return;
        }
        // Reserved where the receiver is read, activated here.
        let reserved = self.make_loan(point, place.clone(), false, span);
        self.body.loans[reserved.0 as usize].two_phase = true;
        self.set_access(point, Access::Reserve);
        let temporary = self.body.loans[reserved.0 as usize].temporary;
        let operands = std::mem::take(&mut self.operands);
        self.push(Fact::Use(temporary));
        let activation = self.push(Fact::Access {
            place: place.clone(),
            access: Access::Mutable,
            span,
            loan: None,
        });
        self.activations.insert(point, activation);
        if place.projection.first() == Some(&Projection::Deref) {
            self.push(Fact::Use(place.local));
        }
        self.operands = operands;
        self.flush();
    }

    /// The use starting at `start` is assigned (`v = x`, `v.a += 1`): it
    /// is read and written here, after the value, or only written when the
    /// assignment is not compound.
    pub(crate) fn assigned(&mut self, start: Position, span: Span, compound: bool) {
        let Some((point, place)) = self.mention_place(start) else {
            return;
        };
        self.set_access(point, Access::None);
        if place.projection.first() == Some(&Projection::Deref) {
            self.push(Fact::Use(place.local));
        } else if !compound {
            // Writing a part of a local's value (`s.r = &y`) does not use
            // what the local holds.
            self.unuse(point, place.local);
        }
        // `v[i] = x` of a `Vec` is `*IndexMut::index_mut(&mut v, i) = x`:
        // its borrow is `&mut` while the index is read, and made again
        // here, after the value.
        if self.overloaded.contains(&point) {
            if let Some(loan) = self.index_loans.get(&point).copied() {
                self.body.loans[loan.0 as usize].mutable = true;
            }
            self.push(Fact::Access {
                place,
                access: Access::Mutable,
                span,
                loan: None,
            });
            return;
        }
        if compound {
            self.push(Fact::Access {
                place: place.clone(),
                access: Access::Read,
                span,
                loan: None,
            });
        }
        self.push(Fact::Access {
            place: place.clone(),
            access: Access::Write,
            span,
            loan: None,
        });
    }

    /// The local whose use starts at `start`: its name and declaration.
    pub(crate) fn mentioned_local(&self, start: Position) -> Option<(ImmutableString, Span)> {
        let (_, place) = self.mention_place(start)?;
        let info = &self.body.locals[place.local.0 as usize];
        Some((info.name.clone(), info.declaration))
    }

    /// The holder used starting at `start` is given a new value (`r = &w`,
    /// spanning `span`): that use neither uses it nor reads through it, and
    /// the holder itself is written here. The holder.
    pub(crate) fn rebound(&mut self, start: Position, span: Span) -> Option<Local> {
        let (point, place) = self.mention_place(start)?;
        self.set_access(point, Access::None);
        self.push(Fact::Access {
            place: Place {
                local: place.local,
                projection: Vec::new(),
            },
            access: Access::Write,
            span,
            loan: None,
        });
        self.unuse(point, place.local);
        Some(place.local)
    }

    /// The use of `local` a mention recorded just before `point` is not a
    /// use after all.
    fn unuse(&mut self, point: Point, local: Local) {
        if point.index > 0 {
            let before = Point {
                block: point.block,
                index: point.index - 1,
            };
            if matches!(self.body.fact(before), Some(Fact::Use(used)) if *used == local) {
                *self.fact_mut(before) = Fact::Nop;
            }
        }
    }

    /// A value spanning `value`, begun at `start`, is stored somewhere
    /// these facts do not follow: what it borrowed is left to rustc when
    /// its type may hold a borrow.
    pub(crate) fn escapes(&mut self, value: Span, start: ValueStart, holds: bool) {
        let made = self.body.loans.len() > start.loans || self.borrowed > start.borrowed;
        if holds && made {
            self.unmodeled(
                "a borrow stored in a place these facts do not follow",
                value,
            );
        }
    }

    /// A value spanning `value`, begun at `start`, that may hold the borrows
    /// it was made from, unless it is one of them (`&v`, `r`).
    pub(crate) fn escapes_unless_direct(&mut self, value: Span, start: ValueStart) {
        if !self.origins.contains_key(&value) {
            self.escapes(value, start, true);
        }
    }

    /// A closure captured locals: it holds them as long as it lives.
    pub(crate) fn captured(&mut self) {
        self.borrowed += 1;
    }

    /// Where the value of a `let` (or an assignment) begins: what it makes
    /// from here is what [`Recorder::claim`] takes.
    pub(crate) fn value_start(&self) -> ValueStart {
        ValueStart {
            loans: self.body.loans.len(),
            borrowed: self.borrowed,
        }
    }

    /// The local `into` takes the value spanning `value`: the loans and
    /// holders that value is flow into it. Anything else the value borrowed
    /// is left to rustc when the value's type may hold a reference.
    pub(crate) fn claim(&mut self, into: Local, value: Span, start: ValueStart, holds: bool) {
        self.push(Fact::Def(into));
        let followed = self.take_origins(into, value);
        let made = self.body.loans.len() > start.loans || self.borrowed > start.borrowed;
        if holds && !followed && made {
            self.unmodeled("a borrow kept in a value these facts do not follow", value);
        }
    }

    /// The value spanning `whole` is one of the values spanning `parts`, as
    /// an `if`'s or a `match`'s is one of its branches'.
    pub(crate) fn either(&mut self, whole: Span, parts: &[Span]) {
        // A branch that diverges, or holds nothing that borrows, adds none.
        let followed: Vec<&Held> = parts
            .iter()
            .filter_map(|part| self.origins.get(part))
            .collect();
        if !followed.is_empty() {
            let origins = followed.into_iter().flatten().copied().collect();
            self.origins.insert(whole, origins);
        }
    }

    // Control flow.

    /// The block control is in, where branches split.
    pub(crate) fn split(&self) -> Block {
        self.current
    }

    /// A branch from `split` starts here.
    pub(crate) fn arm(&mut self, split: Block) {
        let block = self.new_block();
        self.edge(split, block);
        self.current = block;
    }

    /// The branch being read ends here: its end, to join.
    pub(crate) fn arm_end(&self) -> Block {
        self.current
    }

    /// Control continues where the branches ending at `ends` meet.
    pub(crate) fn join(&mut self, ends: &[Block]) {
        let block = self.new_block();
        for &end in ends {
            self.edge(end, block);
        }
        self.current = block;
    }

    /// Control does not reach here (after `return`, `break`, a panic).
    pub(crate) fn diverge(&mut self) {
        self.current = self.new_block();
    }

    /// A loop starts here: its head runs before each iteration.
    pub(crate) fn loop_enter(&mut self) {
        let header = self.new_block();
        self.edge(self.current, header);
        self.current = header;
        self.loops.push(LoopFrame {
            header,
            exits: Vec::new(),
        });
    }

    /// The loop's head may leave it here (`while`'s condition, `for`'s next
    /// element), or go on to the body.
    pub(crate) fn loop_test(&mut self) {
        let here = self.current;
        if let Some(frame) = self.loops.last_mut() {
            frame.exits.push(here);
        }
        self.arm(here);
    }

    /// The depth of the innermost loop being read, from the outermost: what
    /// [`Self::jump`] takes to name it.
    pub(crate) fn loop_depth(&self) -> usize {
        self.loops.len().saturating_sub(1)
    }

    /// `break` (or `continue`) to the loop `depth` from the outermost.
    pub(crate) fn jump(&mut self, depth: usize, breaking: bool) {
        let here = self.current;
        if let Some(frame) = self.loops.get_mut(depth) {
            if breaking {
                frame.exits.push(here);
            } else {
                let header = frame.header;
                self.edge(here, header);
            }
        }
        self.diverge();
    }

    /// The loop's body ends here: it goes back to its head, and control
    /// continues where its exits meet.
    pub(crate) fn loop_exit(&mut self) {
        let Some(frame) = self.loops.pop() else {
            return;
        };
        self.edge(self.current, frame.header);
        self.join(&frame.exits);
    }

    /// The loop being read's head, for a use there each iteration.
    pub(crate) fn loop_header_use(&mut self, local: Local) {
        if let Some(frame) = self.loops.last() {
            let header = frame.header;
            self.body.blocks[header.0 as usize]
                .facts
                .push(Fact::Use(local));
        }
    }

    /// `return` (or `?` failing) leaves the body or closure being read.
    pub(crate) fn ret(&mut self) {
        let here = self.current;
        if let Some(returns) = self.returns.last_mut() {
            returns.push(here);
        }
        self.diverge();
    }

    /// A closure's body starts here: it may run here or not at all, and
    /// its `return`s leave only it.
    pub(crate) fn closure_enter(&mut self) -> Block {
        let split = self.current;
        self.arm(split);
        self.returns.push(Vec::new());
        self.closure_returns.push(Vec::new());
        let outer = std::mem::take(&mut self.loops);
        self.outer_loops.push(outer);
        split
    }

    /// The closure being read returns a value holding `held`, or one these
    /// facts do not follow (`None`).
    pub(crate) fn closure_return(&mut self, held: Option<Held>) {
        if let Some(returns) = self.closure_returns.last_mut() {
            returns.push(held);
        }
    }

    /// The closure's body ends here.
    pub(crate) fn closure_exit(&mut self, split: Block) -> ClosureBody {
        self.flush();
        self.loops = self.outer_loops.pop().unwrap_or_default();
        let mut ends = self.returns.pop().unwrap_or_default();
        ends.push(self.current);
        ends.push(split);
        let blocks = split.0 + 1..self.body.blocks.len() as u32;
        self.join(&ends);
        ClosureBody {
            blocks,
            returns: self.closure_returns.pop().unwrap_or_default(),
        }
    }

    /// The closure whose body is `body` captures the locals `captured`, as
    /// rustc's capture analysis has it from edition 2021: each place its
    /// body uses, cut above an index or a variant, by the weakest borrow its
    /// uses need, or by value when the closure is `move` (cut above a
    /// reference) or its body moves the place. The captures are made here,
    /// where the closure is, as rustc reports them at `span`, and the body
    /// reads them through hidden locals of its own, as a closure's body
    /// reads its environment in rustc.
    ///
    /// `parameters` are where the closure's parameters are declared, and
    /// `tail` what its body's value holds (`None`: a value these facts do
    /// not follow). `copy_of` says whether the part of a captured local
    /// (by its declaration) below a path is `Copy`, when the lowering knows.
    pub(crate) fn captures(
        &mut self,
        body: ClosureBody,
        captured: &[Captured],
        parameters: &[Span],
        tail: Option<Held>,
        moved: bool,
        span: Span,
        copy_of: &dyn Fn(Span, &[Projection]) -> Option<bool>,
    ) -> Closure {
        // Each captured local, and the hidden local its body reads it by.
        let mut upvars = BTreeMap::<Local, (Local, Captured)>::new();
        for capture in captured {
            if let Some(&local) = self.locals.get(&capture.declaration)
                && !upvars.contains_key(&local)
            {
                let name = self.body.locals[local.0 as usize].name.clone();
                let upvar = self.hidden(&name, capture.declaration);
                upvars.insert(local, (upvar, *capture));
            }
        }
        let inside = |point: Point| body.blocks.contains(&point.block.0);
        let mut places = Vec::<CapturePlace>::new();
        let mut mutable = false;
        for block in body.blocks.clone() {
            for fact in &mut self.body.blocks[block as usize].facts {
                match fact {
                    Fact::Use(local) | Fact::Def(local) | Fact::DropUse(local) => {
                        if let Some((upvar, _)) = upvars.get(local) {
                            *local = *upvar;
                        }
                    }
                    Fact::Access { place, access, .. } => {
                        let Some((upvar, capture)) = upvars.get(&place.local) else {
                            continue;
                        };
                        let kind = match access {
                            Access::Read | Access::Length | Access::Shared => {
                                Some(CaptureKind::Shared)
                            }
                            Access::Reserve | Access::Mutable | Access::Write => {
                                Some(CaptureKind::Mutable)
                            }
                            Access::Move => Some(CaptureKind::ByValue),
                            Access::None | Access::StorageDead => None,
                        };
                        mutable |= kind == Some(CaptureKind::Mutable);
                        if let Some(kind) = kind {
                            places.push(capture_place(place, kind, *access, moved, capture));
                        }
                        place.local = *upvar;
                    }
                    Fact::Nop => {}
                }
            }
        }
        for loan in &mut self.body.loans {
            if inside(loan.at)
                && let Some((upvar, _)) = upvars.get(&loan.place.local)
            {
                loan.place.local = *upvar;
            }
        }
        // A borrow the body stores in what it captured, or keeps for
        // `'static`, outlives the call in ways these facts do not follow.
        let mut stores = false;
        for flow in &mut self.body.flows {
            if !inside(flow.at) {
                continue;
            }
            if let Origin::Local(from, _) = &mut flow.from
                && let Some((upvar, _)) = upvars.get(from)
            {
                *from = *upvar;
            }
            if let Some((upvar, _)) = upvars.get(&flow.into) {
                flow.into = *upvar;
                stores = true;
            }
            stores |= self.body.everywhere.contains(&flow.into);
        }
        if stores {
            self.unmodeled("a borrow a closure stores in what it captured", span);
        }
        // The captures, made here. For each captured local: whether it is
        // captured by reference, and the loans it is captured by.
        let mut origins = Vec::new();
        let mut by_local = BTreeMap::<Local, (bool, Vec<Origin>)>::new();
        let mut copy_closure = true;
        let mut by_value = Vec::new();
        for capture in merge(places) {
            let (_, captured) = upvars[&capture.local];
            let place = Place {
                local: capture.local,
                projection: capture.projection,
            };
            let local = place.local;
            let through = place.projection.contains(&Projection::Deref);
            if through {
                self.push(Fact::Use(local));
            }
            if capture.kind == CaptureKind::ByValue {
                by_local.entry(local).or_insert((false, Vec::new())).0 = false;
                let copy = copy_of(captured.declaration, &place.projection)
                    .or(capture.copy)
                    .unwrap_or_else(|| {
                        self.unmodeled(
                            "a part captured by value whose type these facts do not know",
                            span,
                        );
                        true
                    });
                copy_closure &= copy;
                by_value.push((captured.declaration, place.projection.clone(), copy));
                self.push(Fact::Access {
                    place,
                    access: if copy { Access::Read } else { Access::Move },
                    span,
                    loan: None,
                });
                if captured.holder {
                    origins.push(Origin::local(local));
                }
                continue;
            }
            let mutable = capture.kind == CaptureKind::Mutable;
            copy_closure &= !mutable;
            let point = self.push(Fact::Access {
                place: place.clone(),
                access: if mutable {
                    Access::Mutable
                } else {
                    Access::Shared
                },
                span,
                loan: None,
            });
            let loan = self.make_loan(point, place, mutable, span);
            self.body.loans[loan.0 as usize].capture =
                if mutable && through && !captured.declared_mut {
                    Capture::Unique
                } else {
                    Capture::Borrow
                };
            if through || captured.holder {
                let temporary = self.body.loans[loan.0 as usize].temporary;
                self.body.flows.push(Flow {
                    from: Origin::local(local),
                    into: temporary,
                    path: Path::ROOT,
                    at: point,
                });
            }
            origins.push(Origin::Loan(loan));
            by_local
                .entry(local)
                .or_insert((true, Vec::new()))
                .1
                .push(Origin::Loan(loan));
        }
        // What a call keeps: what its returns and its value hold, traced
        // back to what the closure captured.
        let keeps = if stores {
            None
        } else {
            let tracer = Tracer {
                body: &self.body,
                upvars: upvars
                    .iter()
                    .flat_map(|(local, (upvar, _))| [(*local, *local), (*upvar, *local)])
                    .collect(),
                parameters: parameters
                    .iter()
                    .filter_map(|declaration| self.locals.get(declaration).copied())
                    .collect(),
                captures: &by_local,
            };
            let mut kept = Vec::new();
            let mut seen = BTreeSet::new();
            body.returns
                .iter()
                .chain(std::iter::once(&tail))
                .all(|returned| {
                    returned.as_ref().is_some_and(|held| {
                        region::origins(held)
                            .all(|origin| tracer.trace(origin, &mut kept, &mut seen))
                    })
                })
                .then(|| region::everywhere(kept))
        };
        Closure {
            origins: region::everywhere(origins),
            keeps,
            mutable,
            copy: copy_closure,
            by_value,
        }
    }

    /// The closure `key` is `closure`.
    pub(crate) fn set_closure(&mut self, key: crate::item::Key, closure: Closure) {
        self.closures.insert(key, closure);
    }

    /// The closure written at `span` keeps `keeps` when called.
    pub(crate) fn set_closure_keeps(&mut self, span: Span, keeps: Option<Held>) {
        self.closure_keeps.insert(span, keeps);
    }

    /// What a call of the closure written at `span` keeps, if one is
    /// written there.
    pub(crate) fn closure_keeps(&self, span: Span) -> Option<&Option<Held>> {
        self.closure_keeps.get(&span)
    }

    /// The closure `key` of this body, if these facts know it.
    pub(crate) fn closure(&self, key: &crate::item::Key) -> Option<&Closure> {
        self.closures.get(key)
    }

    /// A hidden local holding a value across a loop (`for`'s iterator),
    /// which takes the value spanning `value`.
    pub(crate) fn hold(&mut self, name: &str, value: Span) -> Option<Local> {
        if !self.origins.contains_key(&value) {
            return None;
        }
        let holder = self.hidden(name, value);
        self.push(Fact::Def(holder));
        self.take_origins(holder, value);
        Some(holder)
    }

    /// `into` takes what `from` holds here (a loop's element from its
    /// iterator): for each pair, `into`'s first position takes `from`'s
    /// second.
    pub(crate) fn flow(&mut self, from: Local, into: Local, positions: &[(Path, Path)]) {
        let at = self.push(Fact::Def(into));
        for &(path, position) in positions {
            self.body.flows.push(Flow {
                from: Origin::Local(from, position),
                into,
                path,
                at,
            });
        }
    }

    /// The body is a constant's initializer: the temporaries its value
    /// borrows are promoted to `'static`.
    pub(crate) fn constant(&mut self) {
        self.constant = true;
    }

    /// The body is a function's whose own signature says `parameters`
    /// (each one's name, declaration, borrows' lifetimes, and whether it is
    /// `self`), `output` and `bounds`: what it returns is checked against
    /// them.
    pub(crate) fn own_signature(
        &mut self,
        parameters: &[(ImmutableString, Span, super::signature::Borrows, bool)],
        output: super::signature::Borrows,
        bounds: &[(ImmutableString, ImmutableString)],
    ) {
        let parameters = parameters
            .iter()
            .map(|(name, declaration, borrows, receiver)| {
                (self.local(name, *declaration), borrows.clone(), *receiver)
            })
            .collect();
        self.body.own = Some(super::OwnSignature {
            parameters,
            output,
            bounds: bounds.into(),
        });
    }

    /// A temporary scope starts here: a statement, a match arm, a
    /// condition.
    pub(crate) fn open_temporaries(&mut self) {
        self.temporary_scopes.push(Vec::new());
    }

    /// The temporary scope opened last ends here, as `end` says. A loan of
    /// one of its temporaries still live is rustc's E0716.
    pub(crate) fn close_temporaries(&mut self, end: ScopeEnd) {
        let Some(temporaries) = self.temporary_scopes.pop() else {
            return;
        };
        for mut temporary in temporaries.into_iter().rev() {
            let goes_on = temporary.extendable && !self.pinned.contains(&temporary.local);
            match end {
                ScopeEnd::Let if goes_on => {
                    // At a body's own level it lives to the body's end.
                    if let Some(block) = self.blocks.last_mut() {
                        temporary.ends = None;
                        block.push(temporary);
                    }
                }
                ScopeEnd::Value if goes_on && !self.temporary_scopes.is_empty() => {
                    if temporary.ends.is_none() {
                        // The drop's use goes before the end.
                        if temporary.drops {
                            self.push(Fact::Nop);
                        }
                        temporary.ends = Some(self.push(Fact::Nop));
                    }
                    if let Some(outer) = self.temporary_scopes.last_mut() {
                        outer.push(temporary);
                    }
                }
                _ => self.temporary_dead(temporary),
            }
        }
    }

    /// The temporary drops and ends: here, or where the scope it outlived
    /// ended.
    fn temporary_dead(&mut self, temporary: Temporary) {
        let fact = Fact::Access {
            place: local_place(temporary.local),
            access: Access::StorageDead,
            span: temporary.span,
            loan: None,
        };
        let drop = Fact::DropUse(temporary.local);
        match temporary.ends {
            Some(point) => {
                if temporary.drops {
                    let before = Point {
                        block: point.block,
                        index: point.index - 1,
                    };
                    *self.fact_mut(before) = drop;
                }
                *self.fact_mut(point) = fact;
            }
            None => {
                if temporary.drops {
                    self.push(drop);
                }
                self.push(fact);
            }
        }
    }

    /// A block starts here: what a `let` in it extends ends with it.
    pub(crate) fn open_block(&mut self) {
        self.blocks.push(Vec::new());
    }

    /// Whether the value spanning `span` reads a local or calls something:
    /// a borrow of it is a temporary's, not a constant promoted to
    /// `'static`.
    pub(crate) fn computed(&self, span: Span) -> bool {
        let range = span.start()..=span.end();
        self.mentions.range(range.clone()).next().is_some()
            || self
                .arguments
                .range(range.clone())
                .map(|(end, _)| end)
                .chain(self.calls.range(range))
                .any(|end| !self.constructors.contains(end))
    }

    /// Whether the value spanning `span` is a call's.
    fn is_call(&self, span: Span) -> bool {
        let end = span.end();
        (self.arguments.contains_key(&end) || self.calls.contains(&end))
            && !self.constructors.contains(&end)
    }

    /// A call without arguments ends at `end`.
    pub(crate) fn called(&mut self, end: Position) {
        self.calls.insert(end);
    }

    /// A tuple struct's or variant's constructor ends at `end`.
    pub(crate) fn constructed(&mut self, end: Position) {
        self.constructors.insert(end);
    }

    /// The value spanning `part` is a part of the one spanning `base`, read
    /// through it when it is a `reference`. `made` when that value is made
    /// at run time rather than a constant.
    pub(crate) fn projected(&mut self, part: Span, base: Span, reference: bool, made: bool) {
        // Each step reads from a value written within the part, so a chain
        // of them ends. One spanning what it reads from adds none.
        if base != part && part.start() <= base.start() && base.end() <= part.end() {
            self.projections.insert(part, (base, reference, made));
        }
    }

    /// Whether the part spanning `span` is read from a value made at run
    /// time (`vec![1, 2][0]`), which no constant promotes.
    pub(crate) fn part_of_made(&self, mut span: Span) -> bool {
        while let Some(&(base, _, made)) = self.projections.get(&span) {
            if made {
                return true;
            }
            span = base;
        }
        false
    }

    /// Whether the part spanning `span` is a place in Rust: a part of a
    /// local's value, or one read through a reference. A part of a value a
    /// call made is a part of a temporary.
    pub(crate) fn in_place(&self, mut span: Span) -> bool {
        let mut projected = false;
        while let Some(&(base, reference, _)) = self.projections.get(&span) {
            if reference {
                return true;
            }
            projected = true;
            span = base;
        }
        projected && !self.is_call(span) && self.mentions.contains_key(&span.start())
    }

    /// A borrow (`&mut` when `mutable`) of the value spanning `value`, made
    /// here and held in a temporary: the reference, its loan at its own
    /// region and the temporary's value below it. The value holds `held`.
    /// `extendable` when a `let` may extend it (`&value`), not when it is a
    /// call's receiver. `drops` when the value's drop uses what it holds.
    pub(crate) fn borrow_temporary(
        &mut self,
        value: Span,
        mutable: bool,
        held: &[(Path, Origin)],
        extendable: bool,
        drops: bool,
    ) -> Held {
        let local = self.hidden(super::TEMPORARY, value);
        self.push(Fact::Def(local));
        self.flow_into(local, held);
        let point = self.push(Fact::Access {
            place: local_place(local),
            access: if mutable {
                Access::Mutable
            } else {
                Access::Shared
            },
            span: value,
            loan: None,
        });
        let loan = self.make_loan(point, local_place(local), mutable, value);
        // The reference keeps what the value holds.
        let mut reference = vec![(Path::OWN, Origin::Loan(loan))];
        if !held.is_empty() {
            let temporary = self.body.loans[loan.0 as usize].temporary;
            self.body.flows.push(Flow {
                from: Origin::local(local),
                into: temporary,
                path: Path::ROOT,
                at: point,
            });
            reference.push((Path::of(&[Step::Arg(0)]), Origin::local(local)));
        }
        if let Some(scope) = self.temporary_scopes.last_mut() {
            scope.push(Temporary {
                local,
                span: value,
                extendable,
                drops,
                ends: None,
            });
        }
        reference
    }

    /// A call takes `held`: a temporary one of its loans borrows is no
    /// `let`'s to extend.
    pub(crate) fn pin(&mut self, held: &[(Path, Origin)]) {
        for (_, origin) in held {
            if let Origin::Loan(loan) = origin {
                let local = self.body.loans[loan.0 as usize].place.local;
                if super::is_temporary(&self.body, local) {
                    self.pinned.insert(local);
                }
            }
        }
    }

    /// The locals declared at `declared` end here, with their block: a loan
    /// of one still live is rustc's E0597. The temporaries a `let` in the
    /// block extended end with them. The locals declared at `drops` drop
    /// first, each with a use of what it holds ([`Recorder::drop_use`]).
    /// Each local drops and then ends in reverse order of declaration, as
    /// rustc's do.
    pub(crate) fn storage_dead(&mut self, declared: &[Span], drops: &[Span], end: Span) {
        for temporary in self.blocks.pop().unwrap_or_default().into_iter().rev() {
            self.temporary_dead(temporary);
        }
        self.locals_end(declared, drops, end);
    }

    /// The function body's locals and parameters, declared at `declared`,
    /// end here, the locals declared at `drops` dropping first, as at a
    /// block's end ([`Recorder::storage_dead`]).
    pub(crate) fn body_end(&mut self, declared: &[Span], drops: &[Span], end: Span) {
        self.locals_end(declared, drops, end);
    }

    fn locals_end(&mut self, declared: &[Span], drops: &[Span], end: Span) {
        let mut ending: Vec<Span> = declared.iter().chain(drops).copied().collect();
        ending.sort_by_key(|declaration| std::cmp::Reverse(declaration.start()));
        ending.dedup();
        for declaration in ending {
            if drops.contains(&declaration) {
                self.drop_use(declaration);
            }
            if !declared.contains(&declaration) {
                continue;
            }
            let Some(&local) = self.locals.get(&declaration) else {
                continue;
            };
            self.push(Fact::Access {
                place: Place {
                    local,
                    projection: Vec::new(),
                },
                access: Access::StorageDead,
                span: end,
                loan: None,
            });
        }
    }

    /// The local declared at `declaration` drops here, and its type's
    /// `Drop` may use the borrows it holds.
    pub(crate) fn drop_use(&mut self, declaration: Span) {
        if let Some(&local) = self.locals.get(&declaration) {
            self.push(Fact::DropUse(local));
        }
    }
}

/// What `place` holds, as far as its steps tell without its type: the
/// local's position below references and elements, or all of what is above
/// a field or a variant, whose position only the type tells.
pub(crate) fn place_origin(place: &Place) -> Origin {
    let mut path = Path::ROOT;
    for step in &place.projection {
        match step {
            Projection::Deref | Projection::Index | Projection::ConstantIndex { .. } => {
                path = path.push(Step::Arg(0));
            }
            Projection::Field(_) | Projection::Downcast(_) => {
                return region::narrow(Origin::Local(place.local, path), Path::OWN);
            }
        }
    }
    Origin::Local(place.local, path)
}

/// The place a closure captures for a use of `place` its body makes with
/// `access`, needing `kind`: cut above an index, a slice element or a
/// variant, and for a capture by value above a reference, as rustc's
/// capture analysis cuts it.
fn capture_place(
    place: &Place,
    kind: CaptureKind,
    access: Access,
    moved: bool,
    captured: &Captured,
) -> CapturePlace {
    let mut cut = place
        .projection
        .iter()
        .position(|step| {
            matches!(
                step,
                Projection::Index | Projection::ConstantIndex { .. } | Projection::Downcast(_)
            )
        })
        .unwrap_or(place.projection.len());
    let kind = if moved { CaptureKind::ByValue } else { kind };
    if kind == CaptureKind::ByValue
        && let Some(deref) = place.projection[..cut]
            .iter()
            .position(|step| *step == Projection::Deref)
    {
        cut = deref;
    }
    let whole = cut == place.projection.len();
    // Whether the place is copied when taken by value: a `Copy` local's
    // parts are, and a use of the place itself says so by reading or
    // moving it.
    let copy = if cut == 0 || captured.copy {
        Some(captured.copy)
    } else if whole && access == Access::Move {
        Some(false)
    } else if whole && access == Access::Read {
        Some(true)
    } else {
        None
    };
    CapturePlace {
        local: place.local,
        projection: place.projection[..cut].to_vec(),
        kind,
        copy,
    }
}

/// The captures of `places`, merged as rustc merges them: a place below
/// another captured place is captured with it, by the stronger of the two.
fn merge(places: Vec<CapturePlace>) -> Vec<CapturePlace> {
    let mut merged: Vec<CapturePlace> = Vec::new();
    for mut place in places {
        if let Some(above) = merged.iter_mut().find(|above| {
            above.local == place.local && place.projection.starts_with(&above.projection)
        }) {
            above.kind = above.kind.max(place.kind);
            if above.projection == place.projection {
                above.copy = above.copy.or(place.copy);
            }
            continue;
        }
        merged.retain(|below| {
            let absorbed =
                below.local == place.local && below.projection.starts_with(&place.projection);
            if absorbed {
                place.kind = place.kind.max(below.kind);
            }
            !absorbed
        });
        merged.push(place);
    }
    merged
}

/// Traces what a closure's returned value holds back to what the closure
/// captured.
struct Tracer<'a> {
    body: &'a Body,
    /// The captured locals, and the hidden locals the body reads them by,
    /// each to the captured local.
    upvars: BTreeMap<Local, Local>,
    parameters: BTreeSet<Local>,
    /// For each captured local: whether it is captured by reference, and
    /// the loans of its captures.
    captures: &'a BTreeMap<Local, (bool, Vec<Origin>)>,
}

impl Tracer<'_> {
    /// Adds to `kept` what holding `origin` keeps of what the closure
    /// captured: whether it could tell.
    fn trace(&self, origin: Origin, kept: &mut Vec<Origin>, seen: &mut BTreeSet<Local>) -> bool {
        let captured = |local: Local, kept: &mut Vec<Origin>| {
            if let Some((_, loans)) = self.captures.get(&local) {
                kept.extend(loans.iter().copied());
            }
        };
        match origin {
            Origin::Loan(loan) => {
                let loan = &self.body.loans[loan.0 as usize];
                let root = self.upvars.get(&loan.place.local).copied();
                // A reborrow through what a local refers to keeps what the
                // local holds.
                if loan.place.projection.first() == Some(&Projection::Deref) {
                    return match root {
                        Some(local) => {
                            kept.push(Origin::local(local));
                            captured(local, kept);
                            true
                        }
                        None => self.trace(Origin::local(loan.place.local), kept, seen),
                    };
                }
                // A borrow of a captured place is of the capture's borrow;
                // one of what the closure owns, or of its own local, is
                // refused by rustc.
                match root {
                    Some(local)
                        if self
                            .captures
                            .get(&local)
                            .is_some_and(|(by_reference, _)| *by_reference) =>
                    {
                        captured(local, kept);
                        true
                    }
                    _ => false,
                }
            }
            Origin::Local(local, _) => {
                if !seen.insert(local) {
                    return true;
                }
                if let Some(captured) = self.upvars.get(&local) {
                    kept.push(Origin::local(*captured));
                    return true;
                }
                if self.parameters.contains(&local) {
                    return false;
                }
                if let Some(loan) = self
                    .body
                    .loans
                    .iter()
                    .position(|loan| loan.temporary == local)
                {
                    return self.trace(Origin::Loan(LoanId(loan as u32)), kept, seen);
                }
                self.body
                    .flows
                    .iter()
                    .filter(|flow| flow.into == local)
                    .all(|flow| self.trace(flow.from, kept, seen))
            }
        }
    }
}

/// The whole of `local`.
fn local_place(local: Local) -> Place {
    Place {
        local,
        projection: Vec::new(),
    }
}

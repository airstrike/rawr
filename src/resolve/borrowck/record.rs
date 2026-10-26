//! How `lower_nominal` records a body's facts as it reads the body: one
//! [`Recorder`] per body, told about each use of a local, each borrow,
//! assignment and move, and each branch, loop and jump, in source order.
//!
//! A use of a local is recorded where it is read, as a read of the local;
//! what the lowering then makes of it (a field of it, a borrow of it, an
//! assignment to it, a `&mut self` call on it) refines that record, found
//! by where the use starts.

use super::{
    Access, Block, BlockData, Body, Fact, Flow, Loan, LoanId, Local, LocalInfo, Origin, Place,
    Point, Projection,
};
use crate::{ImmutableString, Position, Span};
use std::collections::{BTreeMap, BTreeSet};

/// A loop being read: where `continue` goes and the blocks that leave it.
struct LoopFrame {
    header: Block,
    exits: Vec<Block>,
}

/// What a value expression began with, so the `let` that takes it can tell
/// what it made: [`Recorder::value_start`].
#[derive(Clone, Copy)]
pub(crate) struct ValueStart {
    loans: usize,
    borrowed: usize,
}

/// The facts of a body being read.
pub(crate) struct Recorder {
    body: Body,
    current: Block,
    locals: BTreeMap<Span, Local>,
    /// The access each use of a local made, by where the use starts.
    mentions: BTreeMap<Position, Point>,
    /// The loans and the holders' loans each expression's value is, by its
    /// span: `&v`, `r`, `if c { &a } else { &b }`.
    origins: BTreeMap<Span, Vec<Origin>>,
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
    /// borrows they are live until the call is made.
    pub(crate) fn call_operands(&mut self, arguments: &[Span]) {
        self.flush();
        for argument in arguments {
            for origin in self.origins.get(argument).cloned().unwrap_or_default() {
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
            Origin::Local(local) => local,
        }
    }

    /// `into` takes the loans of the value spanning `value` here, which
    /// are used here: whether it is a value these facts follow.
    fn take_origins(&mut self, into: Local, value: Span) -> bool {
        let origins = self.origins.get(&value).cloned().unwrap_or_default();
        for origin in &origins {
            let from = self.origin_local(*origin);
            let at = self.push(Fact::Use(from));
            self.body.flows.push(Flow {
                from: Origin::Local(from),
                into,
                at,
            });
        }
        !origins.is_empty()
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
    /// `span`. A holder (a local of a reference type) is used, and the
    /// place read is what it refers to.
    pub(crate) fn mention(&mut self, name: &str, declaration: Span, holder: bool, span: Span) {
        let local = self.local(name, declaration);
        let projection = if holder {
            self.push(Fact::Use(local));
            self.borrowed += 1;
            self.origins.insert(span, vec![Origin::Local(local)]);
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

    /// The use starting at `start` is a match's subject: it reads the
    /// discriminant when it has one, and patterns do not move all of it.
    pub(crate) fn subject(&mut self, start: Position, discriminant: bool) {
        let Some(point) = self.mentioned(start) else {
            return;
        };
        if let Some(Fact::Access {
            access: Access::Read | Access::Shared | Access::Move,
            loan: None,
            ..
        }) = self.body.fact(point)
        {
            self.set_access(
                point,
                if discriminant {
                    Access::Read
                } else {
                    Access::None
                },
            );
            if !discriminant && let Some(Fact::Access { span, .. }) = self.body.fact(point) {
                let span = *span;
                self.unmodeled("the parts of a match's subject its patterns read", span);
            }
            self.subjects.insert(point);
        }
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

    /// A pattern moves the parts `fields` out of the use starting at
    /// `start` (`let S { x, .. } = s;`): each of them, not all of it.
    pub(crate) fn moved_parts(&mut self, start: Position, fields: &[ImmutableString], span: Span) {
        let Some((point, place)) = self.mention_place(start) else {
            return;
        };
        if fields.is_empty() {
            return;
        }
        self.set_access(point, Access::Read);
        for field in fields {
            let mut place = place.clone();
            place.projection.push(Projection::Field(field.clone()));
            self.push(Fact::Access {
                place,
                access: Access::Move,
                span,
                loan: None,
            });
        }
    }

    /// `&place` or `&mut place` (spanning `span`) of the use starting at
    /// `start`: the use becomes the borrow, which makes a loan.
    pub(crate) fn borrow(&mut self, start: Position, span: Span, mutable: bool) -> Option<LoanId> {
        let (point, mut place) = self.mention_place(start)?;
        let through = (place.projection.first() == Some(&Projection::Deref)).then_some(place.local);
        // `&r` and `&mut r` of a holder borrow the holder itself.
        if place.projection == [Projection::Deref] && !self.dereferenced.contains(&point) {
            place.projection.clear();
        }
        // `&v[i]` of a `Vec` is the borrow `Index::index` made.
        let loan = match self.index_loans.get(&point).copied() {
            Some(loan) => {
                self.body.loans[loan.0 as usize].mutable = mutable;
                loan
            }
            None => self.make_loan(point, place, mutable, span),
        };
        self.set_access(
            point,
            if mutable {
                Access::Mutable
            } else {
                Access::Shared
            },
        );
        let temporary = self.body.loans[loan.0 as usize].temporary;
        // A reborrow through a holder (`&mut *r`, `&r.a`) keeps what the
        // holder borrows for as long as it lives.
        if let Some(holder) = through {
            self.body.flows.push(Flow {
                from: Origin::Local(holder),
                into: temporary,
                at: point,
            });
        }
        self.origins.insert(span, vec![Origin::Loan(loan)]);
        Some(loan)
    }

    fn make_loan(&mut self, point: Point, place: Place, mutable: bool, span: Span) -> LoanId {
        let temporary = self.hidden("<borrow>", span);
        self.body.loans.push(Loan {
            place,
            mutable,
            two_phase: false,
            span,
            at: point,
            temporary,
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
        self.push(Fact::Access {
            place: place.clone(),
            access: Access::Mutable,
            span,
            loan: None,
        });
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
        if point.index > 0 {
            let before = Point {
                block: point.block,
                index: point.index - 1,
            };
            if matches!(self.body.fact(before), Some(Fact::Use(local)) if *local == place.local) {
                *self.fact_mut(before) = Fact::Nop;
            }
        }
        Some(place.local)
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
        let origins: Vec<Origin> = parts
            .iter()
            .filter_map(|part| self.origins.get(part))
            .flatten()
            .copied()
            .collect();
        if !origins.is_empty() {
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
        let outer = std::mem::take(&mut self.loops);
        self.outer_loops.push(outer);
        split
    }

    /// The closure's body ends here: the blocks its body was read into.
    pub(crate) fn closure_exit(&mut self, split: Block) -> std::ops::Range<u32> {
        self.flush();
        self.loops = self.outer_loops.pop().unwrap_or_default();
        let mut ends = self.returns.pop().unwrap_or_default();
        ends.push(self.current);
        ends.push(split);
        let body = split.0 + 1..self.body.blocks.len() as u32;
        self.join(&ends);
        body
    }

    /// The closure whose body was read into `blocks` captures the locals
    /// declared at `captured`: it borrows each where it is made, as rustc
    /// reports it at `span`. One that is `move` takes them by value.
    pub(crate) fn captures(
        &mut self,
        blocks: std::ops::Range<u32>,
        captured: &[Span],
        moved: bool,
        span: Span,
    ) {
        let locals: BTreeSet<Local> = captured
            .iter()
            .filter_map(|declaration| self.locals.get(declaration).copied())
            .collect();
        for block in blocks {
            for fact in &mut self.body.blocks[block as usize].facts {
                let Fact::Access {
                    place,
                    access,
                    span: at,
                    loan,
                } = fact
                else {
                    continue;
                };
                if !locals.contains(&place.local) || loan.is_some() {
                    continue;
                }
                *access = match *access {
                    Access::Read | Access::Shared if !moved => Access::Shared,
                    Access::Write | Access::Reserve | Access::Mutable if !moved => Access::Mutable,
                    access => access,
                };
                *at = span;
            }
        }
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
    /// iterator).
    pub(crate) fn flow(&mut self, from: Local, into: Local) {
        let at = self.push(Fact::Def(into));
        self.body.flows.push(Flow {
            from: Origin::Local(from),
            into,
            at,
        });
    }

    /// The locals declared at `declared` end here, with their block: a loan
    /// of one still live is rustc's E0597.
    pub(crate) fn storage_dead(&mut self, declared: &[Span], end: Span) {
        for declaration in declared {
            let Some(&local) = self.locals.get(declaration) else {
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
}

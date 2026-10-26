//! NLL's rule, as stable rustc has it: a loan's region is every point where
//! a local its value flowed into is live (outlives constraints hold at every
//! point), and the loan is in scope at the points reached from where it was
//! made without leaving its region or passing an assignment to its place.

use super::{
    point_count, point_index, Access, Body, Fact, Local, Origin, Point, Points, Projection, Rule,
};

/// The rule of non-lexical lifetimes.
pub(crate) struct Nll;

impl Rule for Nll {
    fn loans_in_scope(&self, body: &Body) -> Vec<Points> {
        let count = point_count(body);
        let index = point_index(body);
        let predecessors = predecessors(body, &index, count);
        let points: Vec<Point> = body.points().collect();
        let mut live_cache: Vec<Option<Points>> = vec![None; body.locals.len()];
        let mut scopes = Vec::with_capacity(body.loans.len());
        for (loan_index, loan) in body.loans.iter().enumerate() {
            // The locals the loan flows into, through holders' flows.
            let mut reached = vec![false; body.locals.len()];
            let mut stack = vec![loan.temporary];
            for flow in &body.flows {
                if flow.from == Origin::Loan(super::LoanId(loan_index as u32)) {
                    stack.push(flow.into);
                }
            }
            while let Some(local) = stack.pop() {
                if std::mem::replace(&mut reached[local.0 as usize], true) {
                    continue;
                }
                for flow in &body.flows {
                    if flow.from == Origin::Local(local) {
                        stack.push(flow.into);
                    }
                }
            }
            let mut region = Points::new(count);
            // Borrowed for `'static`: every point.
            if body
                .everywhere
                .iter()
                .any(|local| reached[local.0 as usize])
            {
                for point in 0..count {
                    region.insert(point);
                }
            }
            for (local, _) in reached.iter().enumerate().filter(|(_, reached)| **reached) {
                let live = live_cache[local].get_or_insert_with(|| {
                    liveness(
                        body,
                        Local(local as u32),
                        &points,
                        &index,
                        &predecessors,
                        count,
                    )
                });
                region.union(live);
            }
            // In scope: reached from the loan's point within its region,
            // stopping after an assignment to its place.
            let mut scope = Points::new(count);
            let mut stack: Vec<Point> = body.successors(loan.at).collect();
            while let Some(point) = stack.pop() {
                let at = index(point);
                if !region.contains(at) || !scope.insert(at) {
                    continue;
                }
                if kills(body.fact(point), loan) {
                    continue;
                }
                stack.extend(body.successors(point));
            }
            scopes.push(scope);
        }
        scopes
    }
}

/// Whether the fact at a point ends the loan: its local is given a new
/// value, or the place it borrows is assigned or ends.
fn kills(fact: Option<&Fact>, loan: &super::Loan) -> bool {
    match fact {
        Some(Fact::Def(local)) => *local == loan.place.local,
        Some(Fact::Access {
            place,
            access: Access::Write | Access::StorageDead,
            ..
        }) => {
            place.local == loan.place.local
                && (place.projection.is_empty()
                    || place.projection.len() <= loan.place.projection.len()
                        && place
                            .projection
                            .iter()
                            .zip(&loan.place.projection)
                            .all(|(a, b)| a == b && *a != Projection::Index))
        }
        _ => false,
    }
}

fn predecessors(body: &Body, index: &impl Fn(Point) -> usize, count: usize) -> Vec<Vec<Point>> {
    let mut predecessors = vec![Vec::new(); count];
    for point in body.points() {
        for successor in body.successors(point) {
            predecessors[index(successor)].push(point);
        }
    }
    predecessors
}

/// The points where `local` is live on entry: a use of it lies ahead on
/// some path, before anything gives it a new value.
fn liveness(
    body: &Body,
    local: Local,
    points: &[Point],
    index: &impl Fn(Point) -> usize,
    predecessors: &[Vec<Point>],
    count: usize,
) -> Points {
    let mut live = Points::new(count);
    let mut stack = Vec::new();
    for &point in points {
        // A read, a borrow or a move of a place is a use of its local, as
        // rustc's liveness has it.
        let used = match body.fact(point) {
            Some(Fact::Use(used)) => *used == local,
            Some(Fact::Access { place, access, .. }) => {
                place.local == local
                    && matches!(
                        access,
                        Access::Read
                            | Access::Shared
                            | Access::Reserve
                            | Access::Mutable
                            | Access::Move
                    )
            }
            _ => false,
        };
        if used {
            stack.push(point);
        }
    }
    while let Some(point) = stack.pop() {
        let at = index(point);
        if !live.insert(at) {
            continue;
        }
        for &predecessor in &predecessors[at] {
            if defines(body, predecessor, local) {
                continue;
            }
            stack.push(predecessor);
        }
    }
    live
}

/// Whether the fact at `point` gives `local` a new value: a `Def`, or a
/// borrow whose hidden local is `local`.
fn defines(body: &Body, point: Point, local: Local) -> bool {
    match body.fact(point) {
        Some(Fact::Def(defined)) => *defined == local,
        Some(Fact::Access {
            loan: Some(loan), ..
        }) => body.loans[loan.0 as usize].temporary == local,
        _ => false,
    }
}

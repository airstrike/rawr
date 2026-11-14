//! The check that a `let x;` not declared `mut` is given its value once,
//! as rustc's borrow checker has it: an assignment on a path where another
//! may have given the local a value is rustc's E0384 there, whichever
//! branch or loop iteration the other was in.

use super::{Access, Block, Body, Fact, Local};
use crate::Span;

/// An assignment to a local that may already have its value.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AssignedTwice {
    /// The local, as the source names it.
    pub(crate) name: String,
    /// The second assignment.
    pub(crate) assignment: Span,
    /// The assignment that may have given it its value before, unless it
    /// is the same one, run again by a loop.
    pub(crate) first: Option<Span>,
}

/// The first assignment in `body`, in the order the body runs, to one of
/// its `single_assignments` locals that may have its value already.
pub(crate) fn check(body: &Body) -> Option<AssignedTwice> {
    if body.single_assignments.is_empty() {
        return None;
    }
    let locals: Vec<Local> = body
        .single_assignments
        .iter()
        .map(|(_, local)| *local)
        .collect();
    // Where each block starts: for each local, an assignment that may
    // have given it its value, until nothing changes.
    let mut entry: Vec<Option<Vec<Option<Span>>>> = vec![None; body.blocks.len()];
    entry[0] = Some(vec![None; locals.len()]);
    let mut work = vec![Block(0)];
    while let Some(block) = work.pop() {
        let mut state = entry[block.0 as usize].clone().expect("a reached block");
        run(body, block, &locals, &mut state, &mut |_, _, _| {});
        for &next in &body.blocks[block.0 as usize].successors {
            let target = &mut entry[next.0 as usize];
            let changed = match target {
                None => {
                    *target = Some(state.clone());
                    true
                }
                Some(known) => {
                    let mut changed = false;
                    for (known, now) in known.iter_mut().zip(&state) {
                        if known.is_none() && now.is_some() {
                            *known = *now;
                            changed = true;
                        }
                    }
                    changed
                }
            };
            if changed && !work.contains(&next) {
                work.push(next);
            }
        }
    }
    let mut first = None;
    for (index, state) in entry.iter().enumerate() {
        let Some(mut state) = state.clone() else {
            continue;
        };
        run(
            body,
            Block(index as u32),
            &locals,
            &mut state,
            &mut |local, assignment, earlier| {
                let twice = AssignedTwice {
                    name: body.locals[local.0 as usize].name.to_string(),
                    assignment,
                    first: Some(earlier).filter(|earlier| *earlier != assignment),
                };
                if first.as_ref().is_none_or(|known: &AssignedTwice| {
                    assignment.start() < known.assignment.start()
                }) {
                    first = Some(twice);
                }
            },
        );
    }
    first
}

/// Runs `block`'s facts over `state` (an assignment that may have given
/// each of `locals` its value), calling `error` at each assignment to one
/// that may have its value, with the earlier assignment.
fn run(
    body: &Body,
    block: Block,
    locals: &[Local],
    state: &mut [Option<Span>],
    error: &mut dyn FnMut(Local, Span, Span),
) {
    let facts = &body.blocks[block.0 as usize].facts;
    for (index, fact) in facts.iter().enumerate() {
        // A `let x;` the body runs again has no value again.
        for (declared, local) in &body.single_assignments {
            if declared.block == block
                && declared.index as usize == index
                && let Some(slot) = locals.iter().position(|known| known == local)
            {
                state[slot] = None;
            }
        }
        let Fact::Access {
            place,
            access: Access::Write,
            span,
            ..
        } = fact
        else {
            continue;
        };
        if !place.projection.is_empty() {
            continue;
        }
        let Some(slot) = locals.iter().position(|local| *local == place.local) else {
            continue;
        };
        if let Some(earlier) = state[slot] {
            error(place.local, *span, earlier);
        }
        state[slot].get_or_insert(*span);
    }
}

//! The move check, as stable rustc's borrow checker has it: a place moved
//! on some path to an access of it, or of a place above or below it, is
//! rustc's E0382 there. Assigning the place or a place above it gives it a
//! value again, and other parts of a partly moved local stay usable.
//!
//! It runs over the same facts as the borrow check: the moves the lowering
//! recorded (a local or a part of it used by value, a pattern's by-value
//! binding, a closure's capture by value) and the body's control flow.

use super::{describe, Access, Block, Body, Fact, Place};
use crate::Span;

/// A use of a place that may have been moved: rustc's E0382.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UseAfterMove {
    /// The place used, as rustc names it.
    pub(crate) name: String,
    /// Where it was moved.
    pub(crate) moved: Span,
    /// The use.
    pub(crate) access: Span,
    /// The use borrows the place: rustc's "borrow of moved value".
    pub(crate) borrowed: bool,
    /// What moved is a part of the place used: rustc's "partially moved".
    pub(crate) partial: bool,
    /// What moved, as rustc names it: the part of a partial move.
    pub(crate) moved_place: String,
    /// Where the moved place's local is declared.
    pub(crate) declaration: Span,
}

/// The first use of a moved place in `body`, in the order the body runs.
pub(crate) fn check(body: &Body) -> Option<UseAfterMove> {
    // Each place the body moves out of, once.
    let mut moved: Vec<(&Place, Span)> = Vec::new();
    for block in &body.blocks {
        for fact in &block.facts {
            if let Fact::Access {
                place,
                access: Access::Move,
                span,
                ..
            } = fact
                && !moved.iter().any(|(known, _)| *known == place)
            {
                moved.push((place, *span));
            }
        }
    }
    if moved.is_empty() {
        return None;
    }
    // Which of them may be moved where each block starts, until nothing
    // changes.
    let mut entry: Vec<Option<Vec<bool>>> = vec![None; body.blocks.len()];
    entry[0] = Some(vec![false; moved.len()]);
    let mut work = vec![Block(0)];
    while let Some(block) = work.pop() {
        let mut state = entry[block.0 as usize].clone().expect("a reached block");
        run(body, block, &moved, &mut state, &mut |_, _, _, _| {});
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
                        if *now && !*known {
                            *known = true;
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
            &moved,
            &mut state,
            &mut |place, span, (known, at), borrowed| {
                // rustc names the moved place when it is above the one
                // used, and the one used when only a part of it moved.
                let above = known.projection.len() < place.projection.len();
                first.get_or_insert_with(|| UseAfterMove {
                    name: describe(body, if above { known } else { place }),
                    moved: at,
                    access: span,
                    borrowed,
                    partial: known.projection.len() > place.projection.len(),
                    moved_place: describe(body, known),
                    declaration: body.locals[known.local.0 as usize].declaration,
                });
            },
        );
        if first.is_some() {
            break;
        }
    }
    first
}

/// A place moved out of, and where.
type Moved<'a> = (&'a Place, Span);

/// Runs `block`'s facts over `state` (which moved places may be moved),
/// calling `error` at each access of a place that may be moved, with the
/// moved place and where it moved, and whether the access borrows it.
fn run(
    body: &Body,
    block: Block,
    moved: &[Moved<'_>],
    state: &mut [bool],
    error: &mut dyn FnMut(&Place, Span, Moved<'_>, bool),
) {
    // Where a place that may be moved and overlaps `place` was moved, if
    // one does (`strict`: one above it).
    let moved_at = |state: &[bool], place: &Place, strict: bool| {
        moved
            .iter()
            .zip(state)
            .find(|((known, _), on)| {
                **on && (!strict || known.projection.len() < place.projection.len())
                    && overlaps(known, place)
            })
            .map(|((known, at), _)| (*known, *at))
    };
    // A closure's body reads its captures through hidden locals of its own,
    // which, with its parameters, are given their values each time it is
    // entered.
    for (entry, local) in &body.closure_entries {
        if *entry == block {
            clear(moved, state, |place| place.local == *local);
        }
    }
    for fact in &body.blocks[block.0 as usize].facts {
        match fact {
            Fact::Def(local) => clear(moved, state, |place| place.local == *local),
            Fact::Access {
                place,
                access,
                span,
                ..
            } => match access {
                Access::None | Access::StorageDead => {}
                Access::Write => {
                    // Assigning part of a moved value is an error; the
                    // place itself, or a place above what was moved, is
                    // given a value again.
                    if let Some(at) = moved_at(state, place, true) {
                        error(place, *span, at, false);
                    }
                    clear(moved, state, |known| {
                        known.projection.len() >= place.projection.len() && overlaps(known, place)
                    });
                }
                _ => {
                    if let Some(at) = moved_at(state, place, false) {
                        let borrowed =
                            matches!(access, Access::Shared | Access::Reserve | Access::Mutable);
                        error(place, *span, at, borrowed);
                    }
                    if *access == Access::Move
                        && let Some(index) = moved.iter().position(|(known, _)| *known == place)
                    {
                        state[index] = true;
                    }
                }
            },
            // A drop moves nothing, and a moved value is not dropped.
            Fact::Use(_) | Fact::DropUse(_) | Fact::Nop => {}
        }
    }
}

/// Marks the moved places `which` picks as having a value again.
fn clear(moved: &[(&Place, Span)], state: &mut [bool], which: impl Fn(&Place) -> bool) {
    for ((known, _), on) in moved.iter().zip(state.iter_mut()) {
        if which(known) {
            *on = false;
        }
    }
}

/// Whether two places overlap: one is the other or a part of it, as the
/// borrow check has it.
fn overlaps(a: &Place, b: &Place) -> bool {
    super::overlap(a, b).is_some()
}

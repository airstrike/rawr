//! A program's iterator as a Rust [`Iterator`], where a crate takes one.
//!
//! The resolver lowers the iterator's chain into a step function whose body
//! runs the chain and suspends at each item (`eval::machine`), as an async
//! body suspends at each `.await`. Each `next()` runs the step on from where
//! it suspended, to its next item or its end, so the chain's closures run as
//! the crate pulls, in Rust's order, and only as far as it pulls.

use super::{Callable, Frame, Poll};
use crate::Dynamic;

/// A program's iterator that native code pulls from, as `rawr::future::
/// Items<T>` fills itertools' `Self: Iterator` and `J: IntoIterator`.
pub struct Items<T> {
    step: Callable,
    state: State,
    item: fn(Dynamic) -> T,
}

/// Where the step function is.
enum State {
    /// Not run yet: the chain's first item is next.
    Fresh,
    /// Suspended after an item.
    Suspended(Frame),
    /// Past the chain's last item.
    Done,
}

impl<T> std::fmt::Debug for Items<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Items")
    }
}

impl<T> Items<T> {
    /// The iterator the step function `step` runs, whose items `item` makes
    /// native values of.
    pub fn new(step: Dynamic, item: fn(Dynamic) -> T) -> crate::RawrResultOf<Self> {
        Ok(Self {
            step: Callable::new(step)?,
            state: State::Fresh,
            item,
        })
    }
}

impl<T> Iterator for Items<T> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        let argument = match std::mem::replace(&mut self.state, State::Done) {
            State::Done => return None,
            State::Fresh => Dynamic::UNIT,
            State::Suspended(frame) => frame.resume(Dynamic::UNIT),
        };
        // A failure in the chain, as a closure's panic, is the pull's, as a
        // panic in a Rust iterator's closure is its `next()`'s.
        let output = self
            .step
            .call(&mut [argument])
            .unwrap_or_else(|error| panic!("{}", super::message(&error)));
        match output.try_cast::<Poll>() {
            Some(Poll::Pending { future, frame }) => {
                self.state = State::Suspended(frame);
                Some((self.item)(future))
            }
            Some(Poll::Ready(_)) | None => None,
        }
    }
}

/// A copy that pulls from the start, as Rust's clone of an iterator that
/// hasn't started does: the chain's sources and closures are the ones it was
/// made with, and its counters start again. itertools' `Product` clones its
/// second iterator before it pulls from it, to walk it once per item of the
/// first. A copy of an ended one has ended too.
impl<T> Clone for Items<T> {
    fn clone(&self) -> Self {
        let state = match self.state {
            State::Fresh => State::Fresh,
            State::Done => State::Done,
            State::Suspended(_) => panic!(
                "rawr: a program's iterator can't be cloned once native code has pulled from it"
            ),
        };
        Self {
            step: self.step.clone(),
            state,
            item: self.item,
        }
    }
}

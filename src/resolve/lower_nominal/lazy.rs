//! Rust's iterators as values at run time. A chain a consumer fuses never
//! becomes one (`iterator.rs`). One passed, returned, stored or kept in a
//! `let mut` is an [`Items`]: its chain as a step function that runs the
//! chain and suspends at each item (`eval::machine`), as an async body
//! suspends at each `.await`. Each `next()` runs the step on from where it
//! suspended, to its next item or its end, so the chain's closures run as
//! the program pulls, in Rust's order, and only as far as it pulls.
//!
//! What a generic body iterates is the value it holds at run time: an
//! `Items`, or an array a native operation made (`invoke`). A program's
//! iterator struct is stepped by its own `next`, through the dispatcher of
//! `Iterator::next`, whose fallback is this module's.

use super::*;

/// What `invoke` does, by its code.
pub(super) const NEW: INT = 0;
pub(super) const NEXT: INT = 1;
pub(super) const PEEK: INT = 2;
pub(super) const LEN: INT = 3;
pub(super) const DRAIN: INT = 4;

/// An iterator's chain as a runtime value.
pub(crate) struct Items {
    /// The step function, while it may make items.
    step: Option<crate::FnPtr>,
    /// Where the step suspended, once it has run.
    frame: Option<crate::future::Frame>,
    /// Items made ahead of `next`: what `peek` saw, or what `len` counted.
    ahead: std::collections::VecDeque<Dynamic>,
}

/// Whether `value` is a runtime iterator.
pub(crate) fn is(value: &Dynamic) -> bool {
    value.is::<Items>()
}

/// A copy goes on by itself from the same item, as Rust's clone of an
/// iterator part way through does: its step's locals are copied.
impl Clone for Items {
    fn clone(&self) -> Self {
        Self {
            step: self.step.clone(),
            frame: self.frame.as_ref().map(crate::future::Frame::duplicate),
            ahead: self.ahead.clone(),
        }
    }
}

impl Items {
    /// The iterator the step function `step` runs.
    fn new(step: crate::FnPtr) -> Self {
        Self {
            step: Some(step),
            frame: None,
            ahead: std::collections::VecDeque::new(),
        }
    }

    /// The next item the step makes, `None` past its last.
    fn pull(&mut self, context: &NativeCallContext<'_>) -> crate::RawrResultOf<Option<Dynamic>> {
        let Some(step) = &self.step else {
            return Ok(None);
        };
        let resume = self
            .frame
            .take()
            .map_or(Dynamic::UNIT, |frame| frame.resume(Dynamic::UNIT));
        let output = step.call_raw(context, None, [resume])?;
        match output.try_cast::<crate::future::Poll>() {
            Some(crate::future::Poll::Pending { future, frame }) => {
                self.frame = Some(frame);
                Ok(Some(future))
            }
            Some(crate::future::Poll::Ready(_)) | None => {
                self.step = None;
                Ok(None)
            }
        }
    }

    fn next(&mut self, context: &NativeCallContext<'_>) -> crate::RawrResultOf<Option<Dynamic>> {
        match self.ahead.pop_front() {
            Some(item) => Ok(Some(item)),
            None => self.pull(context),
        }
    }

    fn peek(&mut self, context: &NativeCallContext<'_>) -> crate::RawrResultOf<Option<Dynamic>> {
        if self.ahead.is_empty()
            && let Some(item) = self.pull(context)?
        {
            self.ahead.push_back(item);
        }
        Ok(self.ahead.front().cloned())
    }

    /// Every item left made ahead, as `len` counts them.
    fn fill(&mut self, context: &NativeCallContext<'_>) -> crate::RawrResultOf<usize> {
        while let Some(item) = self.pull(context)? {
            self.ahead.push_back(item);
        }
        Ok(self.ahead.len())
    }
}

/// `iterator.lazy(value, code, argument)`: `NEW` makes the iterator of the
/// step function `value`; the others take the iterator `value` by
/// reference. `NEXT` and `PEEK` give an `Option` of its next item, `LEN`
/// the number left and `DRAIN` the array of them. An array, as a native
/// operation makes an iterator, gives its elements from the first.
pub(super) fn invoke(context: Option<&NativeCallContext<'_>>, args: &mut FnCallArgs) -> RawrResult {
    let code = args[1].as_int().unwrap_or(-1);
    if code == NEW {
        let step = args[0]
            .take()
            .try_cast::<crate::FnPtr>()
            .ok_or_else(|| invariant("an iterator's step is no function"))?;
        return Ok(Dynamic::from(Items::new(step)));
    }
    if let Some(mut array) = args[0].write_lock::<crate::Array>() {
        return Ok(match code {
            NEXT if array.is_empty() => crate::option::none(),
            NEXT => crate::option::some(array.remove(0)),
            PEEK => array
                .first()
                .cloned()
                .map_or_else(crate::option::none, crate::option::some),
            LEN => Dynamic::from_int(INT::try_from(array.len()).unwrap_or(INT::MAX)),
            _ => Dynamic::from_array(std::mem::take(&mut *array)),
        });
    }
    // A range is std's iterator of its own, which steps where it is.
    if let Some((item, after)) = super::range::next(args[0]) {
        let option =
            |item: Option<Dynamic>| item.map_or_else(crate::option::none, crate::option::some);
        return Ok(match code {
            NEXT => {
                *args[0] = after;
                option(item)
            }
            PEEK => option(item),
            _ => {
                let mut left = Vec::new();
                let mut range = args[0].clone();
                while let Some((Some(item), after)) = super::range::next(&range) {
                    left.push(item);
                    range = after;
                }
                if code == LEN {
                    Dynamic::from_int(INT::try_from(left.len()).unwrap_or(INT::MAX))
                } else {
                    *args[0] = range;
                    Dynamic::from_array(left)
                }
            }
        });
    }
    let context = context.ok_or_else(|| invariant("an iterator is stepped outside a call"))?;
    let mut items = args[0]
        .write_lock::<Items>()
        .ok_or_else(|| invariant("an iterator is not one at runtime"))?;
    let option = |item: Option<Dynamic>| item.map_or_else(crate::option::none, crate::option::some);
    Ok(match code {
        NEXT => option(items.next(context)?),
        PEEK => option(items.peek(context)?),
        LEN => Dynamic::from_int(INT::try_from(items.fill(context)?).unwrap_or(INT::MAX)),
        _ => {
            items.fill(context)?;
            Dynamic::from_array(items.ahead.drain(..).collect())
        }
    })
}

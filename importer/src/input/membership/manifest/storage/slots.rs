//! Append-only scoped owners, with no authority or discovery semantics.

use std::{alloc::Layout, cell::OnceCell};

use rawr_toml::document;

use super::super::{session::Session, Error, Step};

pub(in crate::input::membership::manifest) struct Slots<T> {
    first: Slot<T>,
    opened: bool,
}

struct Slot<T> {
    value: OnceCell<T>,
    next: OnceCell<Vec<Slot<T>>>,
}

impl<T> Slot<T> {
    const fn new() -> Self {
        Self {
            value: OnceCell::new(),
            next: OnceCell::new(),
        }
    }
}

impl<T> Slots<T> {
    pub(in crate::input::membership::manifest) const fn new() -> Self {
        Self {
            first: Slot::new(),
            opened: false,
        }
    }

    /// The exclusive owner borrow permits only one append cursor. Returned
    /// values borrow the owner, not a temporary cursor or admission account.
    pub(in crate::input::membership::manifest) fn writer(&mut self) -> Option<Writer<'_, T>> {
        if self.opened {
            return None;
        }
        self.opened = true;
        Some(Writer { tail: &self.first })
    }
}

pub(in crate::input::membership::manifest) struct Writer<'a, T> {
    tail: &'a Slot<T>,
}

impl<'a, T> Writer<'a, T> {
    pub(in crate::input::membership::manifest) fn push<A: document::Admission>(
        &mut self,
        value: T,
        session: &mut Session<'_, A>,
    ) -> Step<&'a T> {
        session.items(1)?;
        if self.tail.value.get().is_some() {
            let Ok(layout) = Layout::array::<Slot<T>>(1) else {
                return session.fail(Error::Overflow);
            };
            session.admit(document::Request::Allocation {
                bytes: layout.size(),
                alignment: layout.align(),
            })?;
            let mut next = Vec::new();
            if let Err(error) = next.try_reserve_exact(1) {
                return session.fail(Error::Allocation(error));
            }
            #[cfg(test)]
            super::ALLOCATIONS.with(|count| count.set(count.get() + 1));
            next.push(Slot::new());
            if self.tail.next.set(next).is_err() {
                return session.fail(Error::State);
            }
            let Some(next) = self.tail.next.get().and_then(|next| next.first()) else {
                return session.fail(Error::State);
            };
            self.tail = next;
        }
        if self.tail.value.set(value).is_err() {
            return session.fail(Error::State);
        }
        match self.tail.value.get() {
            Some(value) => Ok(value),
            None => session.fail(Error::State),
        }
    }
}

impl<T> Drop for Slot<T> {
    fn drop(&mut self) {
        // Detach each successor before dropping its node. Even a partially
        // populated chain has constant native teardown depth and no allocation.
        let mut next = self.next.take();
        while let Some(mut link) = next {
            next = link.first_mut().and_then(|slot| slot.next.take());
        }
    }
}

#[cfg(test)]
mod tests;

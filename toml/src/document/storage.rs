//! Fallible prospective storage shared by all semantic ownership paths.

use alloc::string::String;
use alloc::vec::Vec;
use core::alloc::Layout;
use core::cell::RefCell;

use crate::decoder::StringBuilder;
use crate::ParseError;

use super::{Admission, Diagnostic, Error, Request, Tag};

pub(super) type Shared<'a, A> = RefCell<Session<'a, A>>;

pub(super) struct Session<'a, A: Admission> {
    admission: &'a mut A,
    pub(super) failure: Option<Error<A::Error>>,
}

impl<'a, A: Admission> Session<'a, A> {
    pub(super) fn new(admission: &'a mut A) -> Self {
        Self {
            admission,
            failure: None,
        }
    }
}

pub(super) fn live<A: Admission>(session: &Shared<'_, A>) -> bool {
    session.borrow().failure.is_none()
}

pub(super) fn admit<A: Admission>(session: &Shared<'_, A>, request: Request) -> bool {
    let mut state = session.borrow_mut();
    if state.failure.is_some() {
        return false;
    }
    match state.admission.admit(request) {
        Ok(()) => true,
        Err(error) => {
            state.failure = Some(Error::Admission(error));
            false
        }
    }
}

pub(super) fn fail<A: Admission>(session: &Shared<'_, A>, error: Error<A::Error>) {
    let mut state = session.borrow_mut();
    if state.failure.is_none() {
        state.failure = Some(error);
    }
}

pub(super) fn invalid<A: Admission>(session: &Shared<'_, A>, diagnostic: Diagnostic) {
    if admit(session, Request::Items(1)) {
        fail(session, Error::Invalid(diagnostic));
    }
}

pub(super) fn raw_error<A: Admission>(session: &Shared<'_, A>, error: ParseError) {
    let tag = if error.description() == "cannot recurse further; max recursion depth met" {
        Tag::Depth
    } else {
        Tag::Syntax
    };
    invalid(
        session,
        Diagnostic {
            tag,
            span: error.unexpected().map(|span| span.start()..span.end()),
            context: error.context().map(|span| span.start()..span.end()),
        },
    );
}

#[derive(Debug)]
pub(super) struct Buffer<T> {
    values: Vec<T>,
    /// Usable capacity admitted by this module, irrespective of allocator slack.
    admitted: usize,
}

impl<T> Default for Buffer<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> Buffer<T> {
    pub(super) const fn new() -> Self {
        Self {
            values: Vec::new(),
            admitted: 0,
        }
    }

    pub(super) fn as_slice(&self) -> &[T] {
        &self.values
    }

    pub(super) fn as_mut_slice(&mut self) -> &mut [T] {
        &mut self.values
    }

    pub(super) fn len(&self) -> usize {
        self.values.len()
    }

    pub(super) fn last_mut(&mut self) -> Option<&mut T> {
        self.values.last_mut()
    }

    pub(super) fn push<A: Admission>(&mut self, value: T, session: &Shared<'_, A>) -> bool {
        if !admit(session, Request::Items(1)) || !self.reserve_one(session) {
            return false;
        }
        self.values.push(value);
        true
    }

    /// Admit exactly `count` slots once, when the final length is known in
    /// advance. Later pushes within it need no growth; beyond it they grow as usual.
    pub(super) fn reserve_exact<A: Admission>(
        &mut self,
        count: usize,
        session: &Shared<'_, A>,
    ) -> bool {
        if count <= self.admitted {
            return true;
        }
        let Ok(layout) = Layout::array::<T>(count) else {
            fail(session, Error::Overflow);
            return false;
        };
        if !admit(
            session,
            Request::Allocation {
                bytes: layout.size(),
                alignment: layout.align(),
            },
        ) {
            return false;
        }
        if self
            .values
            .try_reserve_exact(count - self.values.len())
            .is_err()
        {
            fail(session, Error::Allocation);
            return false;
        }
        self.admitted = count;
        true
    }

    fn reserve_one<A: Admission>(&mut self, session: &Shared<'_, A>) -> bool {
        if self.values.len() < self.admitted {
            return true;
        }
        let Some(next) = self.admitted.max(2).checked_mul(2) else {
            fail(session, Error::Overflow);
            return false;
        };
        let Ok(layout) = Layout::array::<T>(next) else {
            fail(session, Error::Overflow);
            return false;
        };
        if !admit(
            session,
            Request::Allocation {
                bytes: layout.size(),
                alignment: layout.align(),
            },
        ) {
            return false;
        }
        if self
            .values
            .try_reserve_exact(next - self.values.len())
            .is_err()
        {
            fail(session, Error::Allocation);
            return false;
        }
        self.admitted = next;
        true
    }
}

#[derive(Debug)]
pub(super) enum Text<'i> {
    Borrowed(&'i str),
    Owned { value: String, admitted: usize },
}

impl Default for Text<'_> {
    fn default() -> Self {
        Self::Borrowed("")
    }
}

impl<'i> Text<'i> {
    pub(super) fn as_str(&self) -> &str {
        match self {
            Self::Borrowed(value) => value,
            Self::Owned { value, .. } => value,
        }
    }

    pub(super) fn copy<A: Admission>(&self, session: &Shared<'_, A>) -> Option<Self> {
        if !admit(session, Request::String(self.as_str().len())) {
            return None;
        }
        match self {
            Self::Borrowed(value) => Some(Self::Borrowed(value)),
            Self::Owned { value, .. } => {
                let mut result = Self::default();
                if result.append_owned(value, session) {
                    Some(result)
                } else {
                    None
                }
            }
        }
    }

    fn append_owned<A: Admission>(&mut self, append: &str, session: &Shared<'_, A>) -> bool {
        let Some(length) = self.as_str().len().checked_add(append.len()) else {
            fail(session, Error::Overflow);
            return false;
        };
        if !admit(session, Request::String(length)) {
            return false;
        }
        let admitted = match self {
            Self::Borrowed(_) => 0,
            Self::Owned { admitted, .. } => *admitted,
        };
        if length > admitted {
            let Some(next) = admitted.checked_mul(2).map(|size| size.max(length)) else {
                fail(session, Error::Overflow);
                return false;
            };
            if Layout::array::<u8>(next).is_err() {
                fail(session, Error::Overflow);
                return false;
            }
            if !admit(
                session,
                Request::Allocation {
                    bytes: next,
                    alignment: 1,
                },
            ) {
                return false;
            }
            match self {
                Self::Borrowed(previous) => {
                    let mut value = String::new();
                    if value.try_reserve_exact(next).is_err() {
                        fail(session, Error::Allocation);
                        return false;
                    }
                    value.push_str(previous);
                    *self = Self::Owned {
                        value,
                        admitted: next,
                    };
                }
                Self::Owned { value, admitted } => {
                    if value.try_reserve_exact(next - value.len()).is_err() {
                        fail(session, Error::Allocation);
                        return false;
                    }
                    *admitted = next;
                }
            }
        }
        match self {
            Self::Owned { value, .. } => value.push_str(append),
            Self::Borrowed(_) => debug_assert!(append.is_empty()),
        }
        true
    }
}

pub(super) struct Builder<'s, 'a, 'i, A: Admission> {
    pub(super) text: Text<'i>,
    session: &'s Shared<'a, A>,
}

impl<'s, 'a, 'i, A: Admission> Builder<'s, 'a, 'i, A> {
    pub(super) fn new(session: &'s Shared<'a, A>) -> Self {
        Self {
            text: Text::default(),
            session,
        }
    }
}

impl<'i, A: Admission> StringBuilder<'i> for Builder<'_, '_, 'i, A> {
    fn clear(&mut self) {
        self.text = Text::default();
        // The shared failure is intentionally not reset by decoder recovery.
    }

    fn push_str(&mut self, append: &'i str) -> bool {
        if !live(self.session) {
            return false;
        }
        if matches!(self.text, Text::Borrowed("")) {
            if !admit(self.session, Request::String(append.len())) {
                return false;
            }
            self.text = Text::Borrowed(append);
            true
        } else {
            self.text.append_owned(append, self.session)
        }
    }

    fn push_char(&mut self, append: char) -> bool {
        if !live(self.session) {
            return false;
        }
        let mut bytes = [0; 4];
        self.text
            .append_owned(append.encode_utf8(&mut bytes), self.session)
    }
}

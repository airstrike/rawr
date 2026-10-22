// Copyright the Rust Project Developers and tinyvec contributors. MIT OR Apache-2.0.
//! The reached four-slot TinyVec and String storage operations.

use super::{Admission, Session, Step};
use crate::input::membership::manifest::Error;
use rawr_toml::document::Request;
use std::alloc::Layout;

pub(super) enum Buffer<T: Copy + Default> {
    Inline { len: u16, data: [T; 4] },
    Heap { data: Vec<T>, capacity: usize },
}

impl<T: Copy + Default> Buffer<T> {
    pub(super) fn new<A: Admission>(session: &mut Session<'_, A>) -> Step<Self> {
        session.items(4)?;
        Ok(Self::Inline {
            len: 0,
            data: [T::default(); 4],
        })
    }

    pub(super) fn values(&self) -> &[T] {
        match self {
            Self::Inline { len, data } => &data[..usize::from(*len)],
            Self::Heap { data, .. } => data,
        }
    }

    pub(super) fn values_mut(&mut self) -> &mut [T] {
        match self {
            Self::Inline { len, data } => &mut data[..usize::from(*len)],
            Self::Heap { data, .. } => data,
        }
    }

    pub(super) fn push<A: Admission>(
        &mut self,
        value: T,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        match self {
            Self::Inline { len, data } if usize::from(*len) < data.len() => {
                data[usize::from(*len)] = value;
                *len += 1;
            }
            Self::Inline { len, data } => {
                // TinyVec::push drains a full ArrayVec into a Vec with len*2
                // capacity, before pushing the fifth value. It does not reserve5.
                let mut replacement = allocate::<T, _>(usize::from(*len) * 2, session)?;
                session.items(usize::from(*len))?;
                for slot in &mut data[..usize::from(*len)] {
                    replacement.push(std::mem::take(slot));
                }
                replacement.push(value);
                *self = Self::Heap {
                    data: replacement,
                    capacity: 8,
                };
            }
            Self::Heap { data, capacity } => {
                if data.len() == *capacity {
                    let Some(next) = capacity.checked_mul(2) else {
                        return session.fail(Error::Overflow);
                    };
                    let mut replacement = allocate(next, session)?;
                    session.items(data.len())?;
                    replacement.append(data);
                    *data = replacement;
                    *capacity = next;
                }
                data.push(value);
            }
        }
        Ok(())
    }

    pub(super) fn truncate<A: Admission>(
        &mut self,
        new_len: usize,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        match self {
            Self::Inline { len, .. } => {
                let old = usize::from(*len);
                if new_len < old {
                    // Both reached element types are Copy. ArrayVec's
                    // needs_drop branch is false; stale inline slots remain.
                    session.items(1)?;
                    *len = new_len as u16;
                }
            }
            Self::Heap { data, .. } => {
                session.items(1)?;
                data.truncate(new_len);
            }
        }
        Ok(())
    }
}

fn allocate<T, A: Admission>(capacity: usize, session: &mut Session<'_, A>) -> Step<Vec<T>> {
    let Ok(layout) = Layout::array::<T>(capacity) else {
        return session.fail(Error::Overflow);
    };
    session.admit(Request::Allocation {
        bytes: layout.size(),
        alignment: layout.align(),
    })?;
    let mut values = Vec::new();
    if let Err(error) = values.try_reserve_exact(capacity) {
        return session.fail(Error::Allocation(error));
    }
    Ok(values)
}

pub(super) struct Text {
    value: String,
    capacity: usize,
}

impl Text {
    pub(super) fn new() -> Self {
        Self {
            value: String::new(),
            capacity: 0,
        }
    }

    pub(super) fn push<A: Admission>(
        &mut self,
        ch: char,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        let Some(required) = self.value.len().checked_add(ch.len_utf8()) else {
            return session.fail(Error::Overflow);
        };
        session.string(required)?;
        if required > self.capacity {
            let Some(capacity) = self.capacity.checked_mul(2).map(|n| n.max(required).max(8))
            else {
                return session.fail(Error::Overflow);
            };
            let bytes = allocate::<u8, _>(capacity, session)?;
            // An empty Vec is always UTF-8. Keep its admitted storage as String.
            let Ok(mut replacement) = String::from_utf8(bytes) else {
                return session.fail(Error::State);
            };
            session.items(self.value.len())?;
            replacement.push_str(&self.value);
            self.value = replacement;
            self.capacity = capacity;
        }
        session.items(ch.len_utf8())?;
        self.value.push(ch);
        Ok(())
    }

    pub(super) fn into_string(self) -> String {
        self.value
    }
}

/// The pin's safe stable sort has only this predictable scratch allocation.
pub(super) fn sort<A: Admission>(
    values: &mut [(u8, char)],
    session: &mut Session<'_, A>,
) -> Step<()> {
    session.items(1)?;
    let width = std::mem::size_of::<(u8, char)>();
    let len = values.len();
    if len > 4096 / width {
        let capacity = (len - len / 2).max(len.min(8_000_000 / width));
        let Ok(layout) = Layout::array::<(u8, char)>(capacity) else {
            return session.fail(Error::Overflow);
        };
        session.admit(Request::Allocation {
            bytes: layout.size(),
            alignment: layout.align(),
        })?;
    }
    values.sort_by_key(|entry| entry.0);
    Ok(())
}

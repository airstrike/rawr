// Copyright the aho-corasick contributors. MIT OR Unlicense.
//! Standard, case-sensitive prefilter construction; packed search is unreachable.
use std::{
    alloc::Layout,
    sync::{atomic::AtomicUsize, Arc},
};

use rawr_toml::document::{Admission, Request};

use super::storage::{self, Buffer};
use crate::input::membership::manifest::{session::Session, Error, Step};

mod search;

#[cfg(test)]
mod tests;

pub(super) enum Prefilter {
    Memmem(Arc<memchr::memmem::Finder<'static>>),
    Start1(Arc<Start1>),
    Start2(Arc<Start2>),
    Start3(Arc<Start3>),
    Rare1(Arc<Rare1>),
    Rare2(Arc<Rare2>),
    Rare3(Arc<Rare3>),
}
pub(super) struct Start1 {
    pub(super) byte: u8,
}
pub(super) struct Start2 {
    pub(super) bytes: [u8; 2],
}
pub(super) struct Start3 {
    pub(super) bytes: [u8; 3],
}
pub(super) struct Rare1 {
    pub(super) byte: u8,
    pub(super) offset: u8,
}
pub(super) struct Rare2 {
    pub(super) offsets: [u8; 256],
    pub(super) bytes: [u8; 2],
}
pub(super) struct Rare3 {
    pub(super) offsets: [u8; 256],
    pub(super) bytes: [u8; 3],
}

impl Prefilter {
    pub(super) fn try_clone<A: Admission>(&self, session: &mut Session<'_, A>) -> Step<Self> {
        session.items(1)?;
        Ok(match self {
            Self::Memmem(value) => Self::Memmem(Arc::clone(value)),
            Self::Start1(value) => Self::Start1(Arc::clone(value)),
            Self::Start2(value) => Self::Start2(Arc::clone(value)),
            Self::Start3(value) => Self::Start3(Arc::clone(value)),
            Self::Rare1(value) => Self::Rare1(Arc::clone(value)),
            Self::Rare2(value) => Self::Rare2(Arc::clone(value)),
            Self::Rare3(value) => Self::Rare3(Arc::clone(value)),
        })
    }

    pub(super) fn memory_usage(&self) -> usize {
        match self {
            Self::Memmem(finder) => finder.needle().len(),
            _ => 0,
        }
    }
}

pub(super) struct Builder {
    count: usize,
    start: Start,
    rare: Rare,
    memmem_count: usize,
    one: Option<Buffer<u8>>,
    enabled: bool,
}
struct Start {
    bytes: Buffer<bool>,
    count: usize,
    rank: u16,
}
struct Rare {
    set: [u128; 2],
    offsets: [u8; 256],
    available: bool,
    count: usize,
    rank: u16,
}

impl Builder {
    pub(super) fn new<A: Admission>(session: &mut Session<'_, A>) -> Step<Self> {
        // Standard.as_packed() is None: there is no packed builder or its
        // pattern ownership on this closed caller path.
        let start = Start {
            bytes: Buffer::filled(256, false, session)?,
            count: 0,
            rank: 0,
        };
        Ok(Self {
            count: 0,
            start,
            rare: Rare {
                set: [0; 2],
                offsets: [0; 256],
                available: true,
                count: 0,
                rank: 0,
            },
            memmem_count: 0,
            one: None,
            enabled: true,
        })
    }

    pub(super) fn add<A: Admission>(
        &mut self,
        bytes: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        if bytes.is_empty() {
            self.enabled = false;
        }
        if !self.enabled {
            return Ok(());
        }
        self.count += 1;
        self.start.add(bytes);
        self.rare.add(bytes, session)?;
        self.memmem_count += 1;
        self.one = if self.memmem_count == 1 {
            Some(Buffer::copy(bytes, session)?)
        } else {
            None
        };
        Ok(())
    }

    pub(super) fn build<A: Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<Option<Prefilter>> {
        session.items(1)?;
        if !self.enabled {
            return Ok(None);
        }
        if let Some(pattern) = &self.one {
            // Finder::new is an allocation-free finite scan of this admitted
            // needle. into_owned makes exactly one boxed byte-slice copy.
            session.items(1)?;
            let finder = memchr::memmem::Finder::new(pattern.slice());
            storage::admit::<u8, A>(pattern.len(), session)?;
            session.items(pattern.len())?;
            let finder = finder.into_owned();
            return Ok(Some(Prefilter::Memmem(shared(finder, session)?)));
        }
        // Preserve both constructions, including the allocation for the
        // candidate that is subsequently discarded. There is no packed
        // candidate; its source heuristic has patlen=usize::MAX, minlen=0.
        let start = self.start.build(session)?;
        let rare = self.rare.build(session)?;
        Ok(match (start, rare) {
            (Some(start), Some(rare)) => {
                if self.start.count < self.rare.count || self.start.rank <= self.rare.rank + 50 {
                    Some(start)
                } else {
                    Some(rare)
                }
            }
            (Some(start), None) => Some(start),
            (None, Some(rare)) => Some(rare),
            (None, None) => None,
        })
    }
}

impl Start {
    fn add(&mut self, bytes: &[u8]) {
        if self.count > 3 {
            return;
        }
        if let Some(&byte) = bytes.first()
            && !self.bytes.slice()[byte as usize]
        {
            self.bytes.slice_mut()[byte as usize] = true;
            self.count += 1;
            self.rank += u16::from(RANK[byte as usize]);
        }
    }

    fn build<A: Admission>(&self, session: &mut Session<'_, A>) -> Step<Option<Prefilter>> {
        session.items(1)?;
        if self.count > 3 {
            return Ok(None);
        }
        let mut bytes = [0; 3];
        let mut len = 0;
        for b in 0..256 {
            session.items(1)?;
            if !self.bytes.slice()[b] {
                continue;
            }
            if b > 0x7F {
                return Ok(None);
            }
            bytes[len] = b as u8;
            len += 1;
        }
        Ok(match len {
            0 => None,
            1 => Some(Prefilter::Start1(shared(
                Start1 { byte: bytes[0] },
                session,
            )?)),
            2 => Some(Prefilter::Start2(shared(
                Start2 {
                    bytes: [bytes[0], bytes[1]],
                },
                session,
            )?)),
            3 => Some(Prefilter::Start3(shared(Start3 { bytes }, session)?)),
            _ => return session.fail(Error::State),
        })
    }
}

impl Rare {
    fn has(&self, b: u8) -> bool {
        self.set[b as usize / 128] & (1u128 << (b as usize % 128)) != 0
    }

    fn add<A: Admission>(&mut self, bytes: &[u8], session: &mut Session<'_, A>) -> Step<()> {
        session.items(1)?;
        if !self.available {
            return Ok(());
        }
        if self.count > 3 || bytes.len() >= 256 {
            self.available = false;
            return Ok(());
        }
        let Some(&first) = bytes.first() else {
            return Ok(());
        };
        let mut rarest = (first, RANK[first as usize]);
        let mut found = false;
        for (pos, &byte) in bytes.iter().enumerate() {
            session.items(1)?;
            self.offsets[byte as usize] = self.offsets[byte as usize].max(pos as u8);
            if found {
                continue;
            }
            if self.has(byte) {
                found = true;
                continue;
            }
            let rank = RANK[byte as usize];
            if rank < rarest.1 {
                rarest = (byte, rank);
            }
        }
        if !found && !self.has(rarest.0) {
            let byte = rarest.0 as usize;
            self.set[byte / 128] |= 1u128 << (byte % 128);
            self.count += 1;
            self.rank += u16::from(rarest.1);
        }
        Ok(())
    }

    fn build<A: Admission>(&self, session: &mut Session<'_, A>) -> Step<Option<Prefilter>> {
        session.items(1)?;
        if !self.available || self.count > 3 {
            return Ok(None);
        }
        let mut bytes = [0; 3];
        let mut len = 0;
        for b in 0..=255 {
            session.items(1)?;
            if self.has(b) {
                bytes[len] = b;
                len += 1;
            }
        }
        Ok(match len {
            0 => None,
            1 => Some(Prefilter::Rare1(shared(
                Rare1 {
                    byte: bytes[0],
                    offset: self.offsets[bytes[0] as usize],
                },
                session,
            )?)),
            2 => Some(Prefilter::Rare2(shared(
                Rare2 {
                    offsets: self.offsets,
                    bytes: [bytes[0], bytes[1]],
                },
                session,
            )?)),
            3 => Some(Prefilter::Rare3(shared(
                Rare3 {
                    offsets: self.offsets,
                    bytes,
                },
                session,
            )?)),
            _ => return session.fail(Error::State),
        })
    }
}

fn shared<T, A: Admission>(value: T, session: &mut Session<'_, A>) -> Step<Arc<T>> {
    let Ok((layout, _)) = Layout::new::<[AtomicUsize; 2]>().extend(Layout::new::<T>()) else {
        return session.fail(Error::Overflow);
    };
    let layout = layout.pad_to_align();
    session.admit(Request::Allocation {
        bytes: layout.size(),
        alignment: layout.align(),
    })?;
    Ok(Arc::new(value))
}

// Exact aho-corasick 1.1.4 util/byte_frequencies.rs table; lower is rarer.
const RANK: [u8; 256] = [
    55, 52, 51, 50, 49, 48, 47, 46, 45, 103, 242, 66, 67, 229, 44, 43, 42, 41, 40, 39, 38, 37, 36,
    35, 34, 33, 56, 32, 31, 30, 29, 28, 255, 148, 164, 149, 136, 160, 155, 173, 221, 222, 134, 122,
    232, 202, 215, 224, 208, 220, 204, 187, 183, 179, 177, 168, 178, 200, 226, 195, 154, 184, 174,
    126, 120, 191, 157, 194, 170, 189, 162, 161, 150, 193, 142, 137, 171, 176, 185, 167, 186, 112,
    175, 192, 188, 156, 140, 143, 123, 133, 128, 147, 138, 146, 114, 223, 151, 249, 216, 238, 236,
    253, 227, 218, 230, 247, 135, 180, 241, 233, 246, 244, 231, 139, 245, 243, 251, 235, 201, 196,
    240, 214, 152, 182, 205, 181, 127, 27, 212, 211, 210, 213, 228, 197, 169, 159, 131, 172, 105,
    80, 98, 96, 97, 81, 207, 145, 116, 115, 144, 130, 153, 121, 107, 132, 109, 110, 124, 111, 82,
    108, 118, 141, 113, 129, 119, 125, 165, 117, 92, 106, 83, 72, 99, 93, 65, 79, 166, 237, 163,
    199, 190, 225, 209, 203, 198, 217, 219, 206, 234, 248, 158, 239, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255, 255,
    255,
];

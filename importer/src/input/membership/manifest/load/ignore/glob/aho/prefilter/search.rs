// Copyright the aho-corasick contributors. MIT OR Unlicense.
//! Pinned prefilter search followed by Candidate::into_option for overlapping
//! iteration. A confirmed single-pattern match still returns only its start.
use rawr_toml::document::Admission;

use super::Prefilter;
use crate::input::membership::manifest::{session::Session, Error, Step};

impl Prefilter {
    pub(in super::super) fn find<A: Admission>(
        &self,
        haystack: &[u8],
        at: usize,
        session: &mut Session<'_, A>,
    ) -> Step<Option<usize>> {
        // The exact pinned helpers borrow the admitted suffix and use fixed
        // stack search state. Admit the search before invoking any helper.
        session.items(1)?;
        let Some(suffix) = haystack.get(at..) else {
            return session.fail(Error::State);
        };
        let found = match self {
            Self::Memmem(finder) => finder.find(suffix).map(|offset| at + offset),
            Self::Start1(value) => memchr::memchr(value.byte, suffix).map(|offset| at + offset),
            Self::Start2(value) => {
                memchr::memchr2(value.bytes[0], value.bytes[1], suffix).map(|offset| at + offset)
            }
            Self::Start3(value) => {
                memchr::memchr3(value.bytes[0], value.bytes[1], value.bytes[2], suffix)
                    .map(|offset| at + offset)
            }
            Self::Rare1(value) => memchr::memchr(value.byte, suffix).map(|offset| {
                (at + offset)
                    .saturating_sub(usize::from(value.offset))
                    .max(at)
            }),
            Self::Rare2(value) => {
                memchr::memchr2(value.bytes[0], value.bytes[1], suffix).map(|offset| {
                    let position = at + offset;
                    let distance = usize::from(value.offsets[usize::from(haystack[position])]);
                    position.saturating_sub(distance).max(at)
                })
            }
            Self::Rare3(value) => {
                memchr::memchr3(value.bytes[0], value.bytes[1], value.bytes[2], suffix).map(
                    |offset| {
                        let position = at + offset;
                        let distance = usize::from(value.offsets[usize::from(haystack[position])]);
                        position.saturating_sub(distance).max(at)
                    },
                )
            }
        };
        Ok(found)
    }
}

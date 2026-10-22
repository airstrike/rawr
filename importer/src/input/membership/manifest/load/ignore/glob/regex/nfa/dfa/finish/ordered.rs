//! Private flat scratch maps for pinned shuffle/acceleration tree operations.
//! Values are only (), pattern-ID arrays, or fixed accelerator bytes.
use super::{Admission, Session, Step};
use crate::input::membership::manifest::storage::Buffer;

pub(super) struct Map<T>(Buffer<(u32, T)>);

impl<T> Map<T> {
    pub(super) fn new() -> Self {
        Self(Buffer::new())
    }

    pub(super) fn from_sorted(entries: Buffer<(u32, T)>) -> Self {
        Self(entries)
    }

    pub(super) fn len(&self) -> usize {
        self.0.len()
    }

    pub(super) fn entries(&self) -> &[(u32, T)] {
        self.0.as_slice()
    }

    pub(super) fn into_iter(self) -> std::vec::IntoIter<(u32, T)> {
        self.0.into_iter()
    }

    fn find<A: Admission>(
        &self,
        key: u32,
        session: &mut Session<'_, A>,
    ) -> Step<Result<usize, usize>> {
        let (mut low, mut high) = (0, self.0.len());
        while low < high {
            session.items(1)?;
            let middle = low + (high - low) / 2;
            match self.0.as_slice()[middle].0.cmp(&key) {
                std::cmp::Ordering::Less => low = middle + 1,
                std::cmp::Ordering::Greater => high = middle,
                std::cmp::Ordering::Equal => return Ok(Ok(middle)),
            }
        }
        Ok(Err(low))
    }

    pub(super) fn contains<A: Admission>(
        &self,
        key: u32,
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        session.items(1)?;
        Ok(self.find(key, session)?.is_ok())
    }

    pub(super) fn insert<A: Admission>(
        &mut self,
        key: u32,
        value: T,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        match self.find(key, session)? {
            Ok(at) => {
                self.0.as_mut_slice()[at].1 = value;
            }
            Err(at) => {
                // Same admitted flat-map replacement/shift boundary as
                // convert/dependencies/sources.rs, preserving operation order.
                session.items(self.0.len())?;
                session.items(self.0.len() - at)?;
                self.0.push((key, value), session)?;
                self.0.as_mut_slice()[at..].rotate_right(1);
            }
        }
        Ok(())
    }

    pub(super) fn remove<A: Admission>(
        &mut self,
        key: u32,
        session: &mut Session<'_, A>,
    ) -> Step<Option<T>> {
        session.items(1)?;
        let Ok(at) = self.find(key, session)? else {
            return Ok(None);
        };
        session.items(self.0.len() - at)?;
        self.0.as_mut_slice()[at..].rotate_left(1);
        Ok(self.0.pop().map(|(_, value)| value))
    }
}

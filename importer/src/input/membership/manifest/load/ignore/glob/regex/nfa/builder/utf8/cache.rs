// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../../LICENSE-regex-syntax-MIT.
//! regex-automata 0.4.14 thompson/map.rs Utf8BoundedMap.
use super::super::{Buffer, Error, Id, Session, Step, Transition};
use rawr_toml::document::Admission;

const PRIME: u64 = 1099511628211;
const INIT: u64 = 14695981039346656037;
struct Entry {
    version: u16,
    key: Buffer<Transition>,
    value: Id,
}
pub(super) struct Cache {
    version: u16,
    capacity: usize,
    entries: Buffer<Entry>,
}
impl Cache {
    pub(super) fn new() -> Self {
        Self {
            version: 0,
            capacity: 10_000,
            entries: Buffer::new(),
        }
    }
    pub(super) fn clear<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Step<()> {
        session.check()?;
        if self.entries.len() != 0 {
            self.version = self.version.wrapping_add(1);
            if self.version != 0 {
                return Ok(());
            }
        }
        // The source owns the entire replacement before dropping the old
        // table, including its stale transition vectors on version wrap.
        let mut replacement = Buffer::new();
        replacement.reserve_more(self.capacity, session)?;
        for _ in 0..self.capacity {
            replacement.push(
                Entry {
                    version: 0,
                    key: Buffer::new(),
                    value: Id::ZERO,
                },
                session,
            )?;
        }
        session.items(self.entries.len())?;
        self.entries = replacement;
        Ok(())
    }
    pub(super) fn hash<A: Admission>(
        &self,
        key: &[Transition],
        session: &mut Session<'_, A>,
    ) -> Step<usize> {
        session.check()?;
        if self.entries.len() == 0 {
            return session.fail(Error::State);
        }
        let mut hash = INIT;
        session.items(key.len())?;
        for transition in key {
            hash = (hash ^ u64::from(transition.start)).wrapping_mul(PRIME);
            hash = (hash ^ u64::from(transition.end)).wrapping_mul(PRIME);
            hash = (hash ^ u64::from(transition.next.0)).wrapping_mul(PRIME);
        }
        Ok((hash % self.entries.len() as u64) as usize)
    }
    pub(super) fn get<A: Admission>(
        &self,
        key: &[Transition],
        hash: usize,
        session: &mut Session<'_, A>,
    ) -> Step<Option<Id>> {
        session.items(1)?;
        let Some(entry) = self.entries.as_slice().get(hash) else {
            return session.fail(Error::State);
        };
        if entry.version != self.version {
            return Ok(None);
        }
        if entry.key.len() != key.len() {
            return Ok(None);
        }
        session.items(key.len())?;
        if entry.key.as_slice() != key {
            return Ok(None);
        }
        Ok(Some(entry.value))
    }
    pub(super) fn set<A: Admission>(
        &mut self,
        key: Buffer<Transition>,
        hash: usize,
        value: Id,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        let Some(entry) = self.entries.as_mut_slice().get_mut(hash) else {
            return session.fail(Error::State);
        };
        *entry = Entry {
            version: self.version,
            key,
            value,
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};
    fn key<A: Admission>(byte: u8, session: &mut Session<'_, A>) -> Step<Buffer<Transition>> {
        let mut key = Buffer::new();
        key.push(
            Transition {
                start: byte,
                end: byte,
                next: Id::ZERO,
            },
            session,
        )?;
        Ok(key)
    }
    #[test]
    fn clear_invalidates_without_dropping_old_keys_until_wrap() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut cache = Cache::new();
        assert_eq!(cache.entries.len(), 0);
        cache.capacity = 1;
        cache.clear(&mut session).unwrap();
        let original = key(b'a', &mut session).unwrap();
        let hash = cache.hash(original.as_slice(), &mut session).unwrap();
        cache.set(original, hash, Id(7), &mut session).unwrap();
        let probe = key(b'a', &mut session).unwrap();
        assert_eq!(
            cache.get(probe.as_slice(), hash, &mut session).unwrap(),
            Some(Id(7))
        );
        cache.clear(&mut session).unwrap();
        assert_eq!(cache.entries.as_slice()[0].key.len(), 1);
        assert_eq!(
            cache.get(probe.as_slice(), hash, &mut session).unwrap(),
            None
        );
        cache.version = u16::MAX;
        cache.clear(&mut session).unwrap();
        assert_eq!(cache.version, 0);
        assert_eq!(cache.entries.as_slice()[0].key.len(), 0);
        assert_eq!(
            cache.get(probe.as_slice(), hash, &mut session).unwrap(),
            None
        );
        // Preserve the pinned default entry behavior for the empty key.
        assert_eq!(cache.get(&[], 0, &mut session).unwrap(), Some(Id::ZERO));
    }
    #[test]
    fn collisions_require_exact_transition_equality() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut cache = Cache::new();
        cache.capacity = 1;
        cache.clear(&mut session).unwrap();
        cache
            .set(key(b'a', &mut session).unwrap(), 0, Id(1), &mut session)
            .unwrap();
        let different = key(b'b', &mut session).unwrap();
        assert_eq!(cache.hash(different.as_slice(), &mut session).unwrap(), 0);
        assert_eq!(
            cache.get(different.as_slice(), 0, &mut session).unwrap(),
            None
        );
        cache.set(different, 0, Id(2), &mut session).unwrap();
        let previous = key(b'a', &mut session).unwrap();
        assert_eq!(
            cache.get(previous.as_slice(), 0, &mut session).unwrap(),
            None
        );
    }
    struct Count {
        calls: usize,
        refuse: usize,
    }
    impl Admission for Count {
        type Error = ();
        fn admit(&mut self, _: rawr_toml::document::Request) -> Result<(), ()> {
            let call = self.calls;
            self.calls += 1;
            if call == self.refuse {
                Err(())
            } else {
                Ok(())
            }
        }
    }
    fn exercise<A: Admission>(session: &mut Session<'_, A>) -> Step<()> {
        let mut cache = Cache::new();
        cache.capacity = 3;
        cache.clear(session)?;
        let node = key(b'a', session)?;
        let hash = cache.hash(node.as_slice(), session)?;
        cache.get(node.as_slice(), hash, session)?;
        cache.set(node, hash, Id::ZERO, session)?;
        cache.version = u16::MAX;
        cache.clear(session)
    }
    #[test]
    fn refusal_stops_initialization_hashing_and_wrap_replacement() {
        let mut count = Count {
            calls: 0,
            refuse: usize::MAX,
        };
        exercise(&mut Session::new(&mut count)).unwrap();
        for refuse in 0..count.calls {
            let mut count = Count { calls: 0, refuse };
            let mut session = Session::new(&mut count);
            assert!(matches!(exercise(&mut session), Err(Stop::Refused)));
            assert!(matches!(session.check(), Err(Stop::Refused)));
            assert_eq!(count.calls, refuse + 1);
        }
    }
}

// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../../LICENSE-regex-syntax-MIT.
//! Pinned c_unicode_class_reverse_with_suffix and Utf8SuffixMap.
use super::super::{
    fragment::Fragment, Buffer, Builder, Error, Failure, Id, Session, State, Step, Transition,
};
use super::{ranges::Ranges, Class};
use rawr_toml::document::Admission;

#[derive(Clone, Copy, PartialEq, Eq)]
struct Key {
    from: Id,
    start: u8,
    end: u8,
}
struct Entry {
    version: u16,
    key: Key,
    value: Id,
}
pub(in super::super::super) struct Suffix {
    version: u16,
    capacity: usize,
    entries: Buffer<Entry>,
}
impl Suffix {
    pub(in super::super::super) fn new() -> Self {
        Self {
            version: 0,
            capacity: 1_000,
            entries: Buffer::new(),
        }
    }
    fn clear<A: Admission>(&mut self, session: &mut Session<'_, A>) -> Step<()> {
        session.check()?;
        if self.entries.len() != 0 {
            self.version = self.version.wrapping_add(1);
            if self.version != 0 {
                return Ok(());
            }
        }
        let mut replacement = Buffer::new();
        replacement.reserve_more(self.capacity, session)?;
        for _ in 0..self.capacity {
            replacement.push(
                Entry {
                    version: 0,
                    key: Key {
                        from: Id::ZERO,
                        start: 0,
                        end: 0,
                    },
                    value: Id::ZERO,
                },
                session,
            )?;
        }
        session.items(self.entries.len())?;
        self.entries = replacement;
        Ok(())
    }
    fn hash<A: Admission>(&self, key: &Key, session: &mut Session<'_, A>) -> Step<usize> {
        session.items(1)?;
        if self.entries.len() == 0 {
            return session.fail(Error::State);
        }
        let mut hash = 14695981039346656037_u64;
        for value in [
            u64::from(key.from.0),
            u64::from(key.start),
            u64::from(key.end),
        ] {
            hash = (hash ^ value).wrapping_mul(1099511628211);
        }
        Ok((hash % self.entries.len() as u64) as usize)
    }
    fn get<A: Admission>(
        &self,
        key: &Key,
        hash: usize,
        session: &mut Session<'_, A>,
    ) -> Step<Option<Id>> {
        session.items(1)?;
        let Some(entry) = self.entries.as_slice().get(hash) else {
            return session.fail(Error::State);
        };
        Ok((entry.version == self.version && entry.key == *key).then_some(entry.value))
    }
    fn set<A: Admission>(
        &mut self,
        key: Key,
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
impl Builder {
    pub(in super::super::super) fn unicode_reverse<A: Admission>(
        &mut self,
        class: &Class,
        cache: &mut Suffix,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Fragment, Failure>> {
        session.items(1)?;
        if class.is_ascii() {
            return self.unicode_ascii(class, session);
        }
        cache.clear(session)?;
        let union = match self.add(
            State::Union {
                alternates: Buffer::new(),
            },
            session,
        )? {
            Ok(id) => id,
            Err(failure) => return Ok(Err(failure)),
        };
        let end = match self.empty(session)? {
            Ok(fragment) => fragment.end,
            Err(failure) => return Ok(Err(failure)),
        };
        let mut ranges = class.ranges();
        loop {
            session.items(1)?;
            let Some((start, stop)) = ranges.next() else {
                break;
            };
            let mut sequences = Ranges::new(start, stop, session)?;
            while let Some(sequence) = sequences.next(session)? {
                let mut next = end;
                let mut bytes = sequence.as_slice().iter();
                loop {
                    session.items(1)?;
                    let Some(range) = bytes.next() else { break };
                    let key = Key {
                        from: next,
                        start: range.start,
                        end: range.end,
                    };
                    let hash = cache.hash(&key, session)?;
                    if let Some(id) = cache.get(&key, hash, session)? {
                        next = id;
                        continue;
                    }
                    let id = match self.add(
                        State::ByteRange {
                            trans: Transition {
                                start: range.start,
                                end: range.end,
                                next: Id::ZERO,
                            },
                        },
                        session,
                    )? {
                        Ok(id) => id,
                        Err(failure) => return Ok(Err(failure)),
                    };
                    if let Err(failure) = self.patch(id, next, session)? {
                        return Ok(Err(failure));
                    }
                    next = id;
                    cache.set(key, hash, id, session)?;
                }
                if let Err(failure) = self.patch(union, next, session)? {
                    return Ok(Err(failure));
                }
            }
        }
        Ok(Ok(Fragment { start: union, end }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};
    fn class(session: &mut Session<'_, Account>) -> Class {
        let mut chars = Buffer::new();
        for c in ['a', 'é', 'ê', '中', '😀'] {
            chars.push(c, session).unwrap();
        }
        Class::singletons(chars, session).unwrap()
    }
    fn accepts(builder: &Builder, fragment: &Fragment, c: char) -> bool {
        let State::Union { alternates } = &builder.states.as_slice()[fragment.start.0 as usize]
        else {
            return false;
        };
        let mut buffer = [0; 4];
        let bytes = c.encode_utf8(&mut buffer).as_bytes();
        alternates.as_slice().iter().any(|start| {
            let mut id = *start;
            for byte in bytes.iter().rev() {
                let State::ByteRange { trans } = &builder.states.as_slice()[id.0 as usize] else {
                    return false;
                };
                if *byte < trans.start || *byte > trans.end {
                    return false;
                }
                id = trans.next;
            }
            id == fragment.end
        })
    }
    #[test]
    fn reverse_chains_match_canonical_classes_across_cache_reuse() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let class = class(&mut session);
        let mut builder = Builder::new();
        let mut cache = Suffix::new();
        for _ in 0..2 {
            let fragment = builder
                .unicode_reverse(&class, &mut cache, &mut session)
                .unwrap()
                .unwrap();
            for c in ['a', 'b', 'é', 'ê', 'ë', '中', '😀', '😁'] {
                assert_eq!(
                    accepts(&builder, &fragment, c),
                    ['a', 'é', 'ê', '中', '😀'].contains(&c)
                );
            }
        }
    }
    #[test]
    fn collision_and_version_wrap_require_current_exact_key() {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut cache = Suffix::new();
        cache.capacity = 1;
        cache.clear(&mut session).unwrap();
        let key = Key {
            from: Id(3),
            start: 4,
            end: 5,
        };
        let hash = cache.hash(&key, &mut session).unwrap();
        cache.set(key, hash, Id(6), &mut session).unwrap();
        assert_eq!(cache.get(&key, hash, &mut session).unwrap(), Some(Id(6)));
        let other = Key { from: Id(4), ..key };
        assert_eq!(cache.get(&other, hash, &mut session).unwrap(), None);
        cache.clear(&mut session).unwrap();
        assert_eq!(cache.get(&key, hash, &mut session).unwrap(), None);
        cache.version = u16::MAX;
        cache.clear(&mut session).unwrap();
        assert_eq!(cache.get(&key, hash, &mut session).unwrap(), None);
        assert_eq!(
            cache
                .get(
                    &Key {
                        from: Id::ZERO,
                        start: 0,
                        end: 0
                    },
                    0,
                    &mut session
                )
                .unwrap(),
            Some(Id::ZERO)
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
    #[test]
    fn every_refusal_stops_reverse_construction() {
        let mut account = Account;
        let class = class(&mut Session::new(&mut account));
        let mut count = Count {
            calls: 0,
            refuse: usize::MAX,
        };
        Builder::new()
            .unicode_reverse(&class, &mut Suffix::new(), &mut Session::new(&mut count))
            .unwrap()
            .unwrap();
        for refuse in 0..count.calls {
            let mut count = Count { calls: 0, refuse };
            let mut session = Session::new(&mut count);
            assert!(matches!(
                Builder::new().unicode_reverse(&class, &mut Suffix::new(), &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(session.check(), Err(Stop::Refused)));
            assert_eq!(count.calls, refuse + 1);
        }
    }
}

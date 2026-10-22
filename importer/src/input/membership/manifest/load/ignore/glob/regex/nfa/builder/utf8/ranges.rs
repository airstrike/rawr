// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../../LICENSE-regex-syntax-MIT.
//! regex-syntax 0.8.11 utf8.rs scalar splitting with admitted stack ownership.
use super::super::{Buffer, Error, Session, Step};
use rawr_toml::document::Admission;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Range {
    pub(super) start: u8,
    pub(super) end: u8,
}
pub(super) struct Sequence {
    ranges: [Range; 4],
    len: usize,
}
impl Sequence {
    pub(super) fn as_slice(&self) -> &[Range] {
        &self.ranges[..self.len]
    }
}
struct Scalar {
    start: u32,
    end: u32,
}
pub(super) struct Ranges {
    stack: Buffer<Scalar>,
}
impl Ranges {
    pub(super) fn new<A: Admission>(
        start: char,
        end: char,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        let mut stack = Buffer::new();
        stack.push(
            Scalar {
                start: start.into(),
                end: end.into(),
            },
            session,
        )?;
        Ok(Self { stack })
    }

    pub(super) fn next<A: Admission>(
        &mut self,
        session: &mut Session<'_, A>,
    ) -> Step<Option<Sequence>> {
        'top: loop {
            session.items(1)?;
            let Some(mut r) = self.stack.pop() else {
                return Ok(None);
            };
            'inner: loop {
                session.items(1)?;
                if r.start < 0xE000 && r.end > 0xD7FF {
                    self.stack.push(
                        Scalar {
                            start: 0xE000,
                            end: r.end,
                        },
                        session,
                    )?;
                    r.end = 0xD7FF;
                    continue 'inner;
                }
                if r.start > r.end {
                    continue 'top;
                }
                for max in [0x7F, 0x7FF, 0xFFFF] {
                    session.items(1)?;
                    if r.start <= max && max < r.end {
                        self.stack.push(
                            Scalar {
                                start: max + 1,
                                end: r.end,
                            },
                            session,
                        )?;
                        r.end = max;
                        continue 'inner;
                    }
                }
                if r.end <= 0x7F {
                    let mut ranges = [Range { start: 0, end: 0 }; 4];
                    ranges[0] = Range {
                        start: r.start as u8,
                        end: r.end as u8,
                    };
                    return Ok(Some(Sequence { ranges, len: 1 }));
                }
                for i in 1..4 {
                    session.items(1)?;
                    let mask = (1 << (6 * i)) - 1;
                    if (r.start & !mask) != (r.end & !mask) {
                        if (r.start & mask) != 0 {
                            self.stack.push(
                                Scalar {
                                    start: (r.start | mask) + 1,
                                    end: r.end,
                                },
                                session,
                            )?;
                            r.end = r.start | mask;
                            continue 'inner;
                        }
                        if (r.end & mask) != mask {
                            self.stack.push(
                                Scalar {
                                    start: r.end & !mask,
                                    end: r.end,
                                },
                                session,
                            )?;
                            r.end = (r.end & !mask) - 1;
                            continue 'inner;
                        }
                    }
                }
                session.items(8)?;
                let (Some(start), Some(end)) = (char::from_u32(r.start), char::from_u32(r.end))
                else {
                    return session.fail(Error::State);
                };
                let mut start_bytes = [0; 4];
                let mut end_bytes = [0; 4];
                let start = start.encode_utf8(&mut start_bytes).as_bytes();
                let end = end.encode_utf8(&mut end_bytes).as_bytes();
                if start.len() != end.len() || !(2..=4).contains(&start.len()) {
                    return session.fail(Error::State);
                }
                let mut ranges = [Range { start: 0, end: 0 }; 4];
                for i in 0..start.len() {
                    ranges[i] = Range {
                        start: start[i],
                        end: end[i],
                    };
                }
                return Ok(Some(Sequence {
                    ranges,
                    len: start.len(),
                }));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};

    fn collect(start: char, end: char) -> Vec<Sequence> {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut ranges = Ranges::new(start, end, &mut session).unwrap();
        let mut result = Vec::new();
        while let Some(sequence) = ranges.next(&mut session).unwrap() {
            result.push(sequence);
        }
        assert!(ranges.next(&mut session).unwrap().is_none());
        result
    }
    fn matches(sequence: &Sequence, bytes: &[u8]) -> bool {
        sequence.len == bytes.len()
            && sequence
                .as_slice()
                .iter()
                .zip(bytes)
                .all(|(r, b)| r.start <= *b && *b <= r.end)
    }
    #[test]
    fn entire_unicode_covers_each_scalar_once_and_excludes_invalid_encodings() {
        let sequences = collect('\0', char::MAX);
        for scalar in 0..=u32::from(char::MAX) {
            let Some(character) = char::from_u32(scalar) else {
                continue;
            };
            let mut buffer = [0; 4];
            let bytes = character.encode_utf8(&mut buffer).as_bytes();
            assert_eq!(sequences.iter().filter(|s| matches(s, bytes)).count(), 1);
        }
        for bytes in [
            &[0xED, 0xA0, 0x80][..],
            &[0xC0, 0x80],
            &[0xF4, 0x90, 0x80, 0x80],
            &[0xFF],
        ] {
            assert!(!sequences.iter().any(|s| matches(s, bytes)));
        }
    }
    #[test]
    fn splitting_preserves_exact_boundary_membership() {
        for (start, end) in [
            ('z', '\u{85}'),
            ('\u{7F0}', '\u{811}'),
            ('\u{D7FE}', '\u{E001}'),
            ('\u{FFFE}', '\u{10002}'),
            ('\u{10FFFE}', char::MAX),
        ] {
            let sequences = collect(start, end);
            for scalar in u32::from(start).saturating_sub(2)..=u32::from(end).saturating_add(2) {
                let Some(c) = char::from_u32(scalar) else {
                    continue;
                };
                let mut buffer = [0; 4];
                let bytes = c.encode_utf8(&mut buffer).as_bytes();
                assert_eq!(
                    sequences.iter().filter(|s| matches(s, bytes)).count(),
                    usize::from(start <= c && c <= end)
                );
            }
        }
        assert!(collect('z', 'a').is_empty());
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
        let mut ranges = Ranges::new('\0', char::MAX, session)?;
        while ranges.next(session)?.is_some() {}
        Ok(())
    }
    #[test]
    fn every_refusal_stops_range_traversal() {
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

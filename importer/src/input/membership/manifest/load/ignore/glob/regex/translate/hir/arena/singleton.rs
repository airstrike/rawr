// Copyright (c) 2014 The Rust Project Developers. MIT.
// License: ../../../../LICENSE-regex-syntax-MIT.
//! Pinned alternation singleton probes. Failed probes retain their reached
//! work; callers try characters before bytes and perform no deduplication here.
use super::{Buffer, Error, Expr, Kind, Session, Step};
use rawr_toml::document::Admission;

pub(super) fn chars<A: Admission>(
    children: &[Expr],
    session: &mut Session<'_, A>,
) -> Step<Option<Buffer<char>>> {
    session.check()?;
    let mut output = Buffer::new();
    for child in children {
        session.items(1)?;
        let Kind::Literal(literal) = &child.kind else {
            return Ok(None);
        };
        let Some(c) = decode(literal.bytes(), session)? else {
            return Ok(None);
        };
        if literal.bytes().len() != c.len_utf8() {
            return Ok(None);
        }
        output.push(c, session)?;
    }
    Ok(Some(output))
}
// Keep the source's ordered attempts separate: a failed Unicode-class probe
// may have done admitted work before the byte-class fallback begins.
pub(super) fn class_chars<A: Admission>(
    children: &[Expr],
    session: &mut Session<'_, A>,
) -> Step<Option<super::super::super::unicode::Class>> {
    use super::super::super::unicode;
    let mut class = unicode::Class::from_ranges(Buffer::new(), session)?;
    for child in children {
        session.items(1)?;
        match &child.kind {
            Kind::Unicode(other) => class.union(other, session)?,
            Kind::Bytes(other) => {
                let Some(converted) = other.to_unicode(session)? else {
                    return Ok(None);
                };
                class.union(&converted, session)?;
            }
            _ => return Ok(None),
        }
    }
    Ok(Some(class))
}

pub(super) fn class_bytes<A: Admission>(
    children: &[Expr],
    session: &mut Session<'_, A>,
) -> Step<Option<super::super::super::bytes::Class>> {
    use super::super::super::bytes;
    session.check()?;
    let mut class = bytes::Class::empty();
    for child in children {
        session.items(1)?;
        match &child.kind {
            Kind::Unicode(other) => {
                let Some(converted) = other.to_bytes(session)? else {
                    return Ok(None);
                };
                class.union(&converted, session)?;
            }
            Kind::Bytes(other) => class.union(other, session)?,
            _ => return Ok(None),
        }
    }
    Ok(Some(class))
}

pub(super) fn bytes<A: Admission>(
    children: &[Expr],
    session: &mut Session<'_, A>,
) -> Step<Option<Buffer<u8>>> {
    session.check()?;
    let mut output = Buffer::new();
    for child in children {
        session.items(1)?;
        let Kind::Literal(literal) = &child.kind else {
            return Ok(None);
        };
        let [byte] = literal.bytes() else {
            return Ok(None);
        };
        output.push(*byte, session)?;
    }
    Ok(Some(output))
}
// regex-syntax debug::utf8_decode inspects only the first encoded scalar.
// Both empty input and invalid first sequences reject the singleton probe.
fn decode<A: Admission>(bytes: &[u8], session: &mut Session<'_, A>) -> Step<Option<char>> {
    session.items(1)?;
    let Some(&first) = bytes.first() else {
        return Ok(None);
    };
    let length = if first <= 0x7f {
        1
    } else if first & 0xc0 == 0x80 {
        return Ok(None);
    } else if first <= 0xdf {
        2
    } else if first <= 0xef {
        3
    } else if first <= 0xf7 {
        4
    } else {
        return Ok(None);
    };
    if length > bytes.len() {
        return Ok(None);
    }
    if length == 1 {
        return Ok(Some(char::from(first)));
    }
    session.items(length)?;
    let Ok(decoded) = std::str::from_utf8(&bytes[..length]) else {
        return Ok(None);
    };
    match decoded.chars().next() {
        Some(c) => Ok(Some(c)),
        None => session.fail(Error::State),
    }
}

#[cfg(test)]
mod tests {
    use super::super::super::{Literal, Look};
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};
    fn literal(bytes: &[u8]) -> Expr {
        let mut account = Account;
        let mut session = Session::new(&mut account);
        Expr::literal(Literal::copy(bytes, &mut session).unwrap(), &mut session).unwrap()
    }
    #[test]
    fn probes_preserve_duplicates_and_distinguish_bytes_from_scalars() {
        let children = [literal(b"a"), literal("é".as_bytes()), literal(b"a")];
        assert_eq!(
            chars(&children, &mut Session::new(&mut Account))
                .unwrap()
                .unwrap()
                .as_slice(),
            &['a', 'é', 'a']
        );
        assert!(bytes(&children, &mut Session::new(&mut Account))
            .unwrap()
            .is_none());
        let children = [literal(&[0xc3]), literal(&[0xa9])];
        assert!(chars(&children, &mut Session::new(&mut Account))
            .unwrap()
            .is_none());
        assert_eq!(
            bytes(&children, &mut Session::new(&mut Account))
                .unwrap()
                .unwrap()
                .as_slice(),
            &[0xc3, 0xa9]
        );
        let children = [literal(b"ab")];
        assert!(chars(&children, &mut Session::new(&mut Account))
            .unwrap()
            .is_none());
    }
    #[test]
    fn first_scalar_decoder_preserves_utf8_boundaries() {
        for c in [
            '\0',
            '\u{7f}',
            '\u{80}',
            '\u{7ff}',
            '\u{800}',
            '\u{d7ff}',
            '\u{e000}',
            '\u{ffff}',
            '\u{10000}',
            '\u{10ffff}',
        ] {
            let mut bytes = [0; 4];
            assert_eq!(
                decode(
                    c.encode_utf8(&mut bytes).as_bytes(),
                    &mut Session::new(&mut Account)
                )
                .unwrap(),
                Some(c)
            );
        }
        for bytes in [
            &[][..],
            &[0x80],
            &[0xc0, 0x80],
            &[0xed, 0xa0, 0x80],
            &[0xf4, 0x90, 0x80, 0x80],
            &[0xf8],
            &[0xe2, 0x82],
        ] {
            assert_eq!(
                decode(bytes, &mut Session::new(&mut Account)).unwrap(),
                None
            );
        }
        assert_eq!(
            decode(&[b'a', 0xff], &mut Session::new(&mut Account)).unwrap(),
            Some('a')
        );
    }
    struct Count {
        calls: usize,
        refuse: usize,
    }
    fn byte_class(lower: u8, upper: u8) -> Expr {
        use super::super::super::super::bytes;
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut class = bytes::Class::empty();
        class
            .push(bytes::Range::new(lower, upper), &mut session)
            .unwrap();
        Expr::bytes(class, &mut session).unwrap()
    }
    fn unicode_class(lower: char, upper: char) -> Expr {
        use super::super::super::super::unicode;
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let mut ranges = Buffer::new();
        ranges
            .push(unicode::Range::new(lower, upper), &mut session)
            .unwrap();
        let class = unicode::Class::from_ranges(ranges, &mut session).unwrap();
        Expr::unicode(class, &mut session).unwrap()
    }
    #[test]
    fn class_probes_merge_mixed_ascii_and_reject_non_ascii_conversion() {
        let children = [byte_class(b'a', b'c'), unicode_class('c', 'f')];
        let mut account = Account;
        let mut session = Session::new(&mut account);
        let chars = class_chars(&children, &mut session).unwrap().unwrap();
        let converted = chars.to_bytes(&mut session).unwrap().unwrap();
        let bytes = class_bytes(&children, &mut session).unwrap().unwrap();
        for byte in 0..=255 {
            let expected = (b'a'..=b'f').contains(&byte);
            assert_eq!(converted.contains(byte), expected);
            assert_eq!(bytes.contains(byte), expected);
        }
        let children = [byte_class(b'a', b'c'), byte_class(128, 255)];
        assert!(class_chars(&children, &mut session).unwrap().is_none());
        assert!(class_bytes(&children, &mut session).unwrap().is_some());
        let children = [unicode_class('a', 'c'), unicode_class('é', 'ê')];
        assert!(class_chars(&children, &mut session).unwrap().is_some());
        assert!(class_bytes(&children, &mut session).unwrap().is_none());
        let children = [byte_class(b'a', b'c'), literal(b"x")];
        assert!(class_chars(&children, &mut session).unwrap().is_none());
        assert!(class_bytes(&children, &mut session).unwrap().is_none());
    }
    #[test]
    fn class_probe_refusal_prevents_later_fallback_work() {
        let children = [unicode_class('a', 'c'), byte_class(128, 255)];
        let exercise = |session: &mut Session<'_, Count>| -> Step<()> {
            assert!(class_chars(&children, session)?.is_none());
            assert!(class_bytes(&children, session)?.is_some());
            Ok(())
        };
        let mut count = Count {
            calls: 0,
            refuse: usize::MAX,
        };
        exercise(&mut Session::new(&mut count)).unwrap();
        for refuse in 0..count.calls {
            let mut count = Count { calls: 0, refuse };
            let mut session = Session::new(&mut count);
            assert!(matches!(exercise(&mut session), Err(Stop::Refused)));
            assert!(matches!(
                class_bytes(&children, &mut session),
                Err(Stop::Refused)
            ));
            assert_eq!(count.calls, refuse + 1);
        }
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
    fn failed_probe_work_is_admitted_and_refusal_prevents_fallback() {
        let children = [
            literal(b"a"),
            Expr::look(Look::End, &mut Session::new(&mut Account)).unwrap(),
        ];
        let mut count = Count {
            calls: 0,
            refuse: usize::MAX,
        };
        assert!(chars(&children, &mut Session::new(&mut count))
            .unwrap()
            .is_none());
        for refuse in 0..count.calls {
            let mut count = Count { calls: 0, refuse };
            let mut session = Session::new(&mut count);
            assert!(matches!(chars(&children, &mut session), Err(Stop::Refused)));
            assert!(matches!(bytes(&children, &mut session), Err(Stop::Refused)));
            assert_eq!(count.calls, refuse + 1);
        }
    }
}

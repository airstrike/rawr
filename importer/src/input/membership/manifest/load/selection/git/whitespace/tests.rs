use super::{dfa::Dfa, Aligned, Owner, State, TABLE};
use crate::input::membership::{
    manifest::{session::Session, Error},
    Budget,
};
use crate::{input, node};
use rawr_toml::document::{Admission, Request};

#[derive(Default)]
struct Count {
    calls: usize,
    items: usize,
    allocations: usize,
    refuse: Option<usize>,
}

impl Admission for Count {
    type Error = usize;
    fn admit(&mut self, request: Request) -> Result<(), Self::Error> {
        let at = self.calls;
        self.calls += 1;
        if self.refuse == Some(at) {
            return Err(at);
        }
        match request {
            Request::Items(amount) => self.items += amount,
            Request::Allocation { .. } => self.allocations += 1,
            Request::String(_) => {}
        }
        Ok(())
    }
}

fn owner() -> Owner {
    Owner::admit(&mut Session::new(&mut Count::default())).unwrap()
}

fn trim<'a>(
    owner: &mut Owner,
    bytes: &'a [u8],
    account: &mut Count,
) -> Result<&'a [u8], Error<usize>> {
    let mut session = Session::new(account);
    let result = owner.trim_end(bytes, &mut session);
    session.finish(result)
}

#[test]
fn fixed_unicode_table_preserves_arbitrary_bytes_and_greedy_reverse_eoi() {
    let mut owner = owner();
    let mut count = Count::default();
    let spaces = "\t\n\u{b}\u{c}\r \u{85}\u{a0}\u{1680}\u{2000}\u{2001}\u{2002}\u{2003}\u{2004}\u{2005}\u{2006}\u{2007}\u{2008}\u{2009}\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}";
    for space in spaces.chars() {
        let mut bytes = b"\xff/path ".to_vec();
        let mut utf8 = [0; 4];
        bytes.extend_from_slice(space.encode_utf8(&mut utf8).as_bytes());
        assert_eq!(trim(&mut owner, &bytes, &mut count).unwrap(), b"\xff/path");
    }
    let cases: &[(&[u8], &[u8])] = &[
        (b"", b""),
        (b" \t\n", b""),
        (spaces.as_bytes(), b""),
        (b"path", b"path"),
        (b"  path  ", b"  path"),
        (b"\xff \t", b"\xff"),
        (b" \xff", b" \xff"),
        (b"\xe2 \t", b"\xe2"),
        (b"path\0 ", b"path\0"),
        ("path\u{200b}".as_bytes(), "path\u{200b}".as_bytes()),
        ("path\u{feff}".as_bytes(), "path\u{feff}".as_bytes()),
    ];
    for &(bytes, expected) in cases {
        let output = trim(&mut owner, bytes, &mut count).unwrap();
        assert_eq!(output, expected, "input {bytes:?}");
        assert_eq!(output.as_ptr(), bytes.as_ptr());
    }
    // Each possible single byte is covered, including invalid UTF-8.
    for byte in 0..=255u8 {
        let input = [byte];
        let expected = if byte == b' ' || (b'\t'..=b'\r').contains(&byte) {
            0
        } else {
            1
        };
        assert_eq!(
            trim(&mut owner, &input, &mut count).unwrap().len(),
            expected
        );
    }
    assert_eq!(count.allocations, 0);
}

#[test]
fn empty_first_use_validates_once_then_reuses_in_original_account() {
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::with_limits(100_000, 0, 1024);
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut session = Session::new(&mut budget);
    let mut owner = Owner::admit(&mut session).unwrap();
    let result = owner.trim_end(b"", &mut session);
    assert_eq!(session.finish(result).unwrap(), b"");
    let cold = budget.allocation.usage().items;
    let mut session = Session::new(&mut budget);
    let result = owner.trim_end(b"", &mut session);
    assert_eq!(session.finish(result).unwrap(), b"");
    let warm = budget.allocation.usage().items - cold;
    assert!(cold > 1_000 && warm < 20, "cold {cold}, warm {warm}");
    assert_eq!(budget.allocation.usage().bytes, 0);
    assert!(matches!(owner.state, State::Ready(_)));
    assert_eq!(Owner::layout(), std::alloc::Layout::new::<Owner>());
}

#[test]
fn refusal_is_terminal_during_decode_or_search_even_with_fresh_session() {
    let mut baseline = Count::default();
    trim(&mut owner(), b"path\t", &mut baseline).unwrap();
    for at in [0, baseline.calls / 2, baseline.calls - 1] {
        let mut owner = owner();
        let mut account = Count {
            refuse: Some(at),
            ..Count::default()
        };
        assert!(matches!(trim(&mut owner, b"path\t", &mut account),
            Err(Error::Admission(cause)) if cause == at));
        assert!(matches!(owner.state, State::Refused));
        let mut fresh = Count::default();
        assert!(matches!(
            trim(&mut owner, b"path\t", &mut fresh),
            Err(Error::State)
        ));
        assert_eq!(fresh.calls, 0);
    }
    let mut owner = owner();
    trim(&mut owner, b"", &mut Count::default()).unwrap();
    let mut account = Count {
        refuse: Some(1),
        ..Count::default()
    };
    assert!(matches!(
        trim(&mut owner, b"path ", &mut account),
        Err(Error::Admission(1))
    ));
    let mut fresh = Count::default();
    assert!(matches!(
        trim(&mut owner, b"path ", &mut fresh),
        Err(Error::State)
    ));
    assert_eq!(fresh.calls, 0);
}

#[test]
fn safe_runtime_decoder_rejects_corrupt_header_transition_start_match_and_special() {
    // Offsets belong to the exact checked-in bstr artifact, not a generated DFA.
    for (offset, bytes) in [
        (32, 0u32.to_le_bytes()),       // endian check
        (312, 1u32.to_le_bytes()),      // unaligned transition target
        (3016, u32::MAX.to_le_bytes()), // universal anchored ID becomes absent: valid wire
        (3020, u32::MAX.to_le_bytes()), // first start ID outside the table
        (3076, u32::MAX.to_le_bytes()), // first match length exceeds IDs
        (3164, u32::MAX.to_le_bytes()), // special max exceeds StateID
    ] {
        let mut table = Aligned {
            _align: [],
            bytes: TABLE.bytes,
        };
        table.bytes[offset..offset + 4].copy_from_slice(&bytes);
        let mut count = Count::default();
        let mut session = Session::new(&mut count);
        let result = Dfa::decode(&table.bytes, &mut session);
        // The universal optional ID mutation is intentionally valid and must
        // still get full table validation rather than a hash-only shortcut.
        if offset == 3016 {
            assert!(session.finish(result).is_ok());
        } else {
            assert!(
                matches!(session.finish(result), Err(Error::State)),
                "offset {offset}"
            );
        }
    }
    let mut table = Aligned {
        _align: [],
        bytes: TABLE.bytes,
    };
    table.bytes[56] = 255; // class exceeds classes[255]+2
    let mut count = Count::default();
    let mut session = Session::new(&mut count);
    let result = Dfa::decode(&table.bytes, &mut session);
    assert!(matches!(session.finish(result), Err(Error::State)));
}

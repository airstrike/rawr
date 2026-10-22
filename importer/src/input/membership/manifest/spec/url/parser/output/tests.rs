use rawr_toml::document;

use super::{offset, Output, Overflow, Set};
use crate::input::membership::manifest::{session, Error, Step, Stop};

mod reference;
mod resources;

#[derive(Default)]
struct Account {
    requests: Vec<document::Request>,
    refusal: Option<usize>,
}

impl document::Admission for Account {
    type Error = usize;

    fn admit(&mut self, request: document::Request) -> Result<(), usize> {
        let index = self.requests.len();
        self.requests.push(request);
        if self.refusal == Some(index) {
            Err(index)
        } else {
            Ok(())
        }
    }
}

fn exercise<A: document::Admission>(session: &mut session::Session<'_, A>) -> Step<()> {
    let mut output = Output::new();
    output.append("file://host/", session)?;
    output.encode("é💩 %?#", Set::Path, session)?;
    output.insert(7, "user@", session)?;
    output.remove(7..12, session)?;
    assert_eq!(output.offset(session)?, Ok(40));
    output.push('終', session)?;
    assert_eq!(output.pop(session)?, Some('終'));
    output.truncate(12, session)?;
    output.clear(session)?;
    output.append("a", session)?;
    assert_eq!(output.as_str(session)?, "a");
    Ok(())
}

#[test]
fn edits_match_string_operations_without_allocating_replacement_substrings() {
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let mut output = Output::new();
    let mut expected = String::new();
    for text in ["é", "💩", "終", "ascii", ""] {
        output.append(text, &mut session).unwrap();
        expected.push_str(text);
        assert_eq!(output.as_str(&mut session).unwrap(), expected);
    }
    for (index, text) in [(2, "/."), (0, "前"), (7, ""), (11, "後")] {
        output.insert(index, text, &mut session).unwrap();
        expected.insert_str(index, text);
        assert_eq!(output.as_str(&mut session).unwrap(), expected);
    }
    output.remove(3..7, &mut session).unwrap();
    expected.replace_range(3..7, "");
    assert_eq!(output.as_str(&mut session).unwrap(), expected);
    while !expected.is_empty() {
        assert_eq!(output.pop(&mut session).unwrap(), expected.pop());
        assert_eq!(output.as_str(&mut session).unwrap(), expected);
    }
    assert_eq!(output.pop(&mut session).unwrap(), None);
    output.truncate(usize::MAX, &mut session).unwrap();
    output.clear(&mut session).unwrap();
    session.finish(Ok(())).unwrap();
}

#[test]
fn malformed_edit_boundaries_latch_state_and_leave_content_unchanged() {
    for operation in 0..5 {
        let mut account = Account::default();
        let mut session = session::Session::new(&mut account);
        let mut output = Output::new();
        output.append("éx", &mut session).unwrap();
        let result = match operation {
            0 => output.truncate(1, &mut session),
            1 => output.insert(1, "z", &mut session),
            2 => output.remove(1..2, &mut session),
            3 => output.remove(std::ops::Range { start: 2, end: 1 }, &mut session),
            _ => output.remove(0..4, &mut session),
        };
        assert!(matches!(result, Err(Stop::Refused)));
        assert_eq!(output.as_bytes(), "éx".as_bytes());
        assert!(matches!(output.clear(&mut session), Err(Stop::Refused)));
        assert!(matches!(output.as_str(&mut session), Err(Stop::Refused)));
        assert!(matches!(session.finish(Ok(())), Err(Error::State)));
    }
}

#[test]
fn semantic_u32_offsets_remain_distinct_from_layout_arithmetic_refusal() {
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let mut output = Output::new();
    assert_eq!(output.offset(&mut session).unwrap(), Ok(0));
    output.append("é", &mut session).unwrap();
    assert_eq!(output.offset(&mut session).unwrap(), Ok(2));
    assert_eq!(
        offset(u32::MAX as usize, &mut session).unwrap(),
        Ok(u32::MAX)
    );
    if let Ok(length) = usize::try_from(u64::from(u32::MAX) + 1) {
        assert_eq!(offset(length, &mut session).unwrap(), Err(Overflow));
        // Semantic offset failure does not poison a later admitted trial.
        output.append("z", &mut session).unwrap();
    }
    assert!(matches!(
        output.grow(usize::MAX, &mut session),
        Err(Stop::Refused)
    ));
    assert!(matches!(output.offset(&mut session), Err(Stop::Refused)));
    assert!(matches!(session.finish(Ok(())), Err(Error::Overflow)));
}

#[test]
fn borrowed_string_conversion_checks_utf8_and_latches_invalid_internal_data() {
    let mut account = Account::default();
    let mut session = session::Session::new(&mut account);
    let mut output = Output::new();
    output.bytes.push(0xff, &mut session).unwrap();
    assert!(matches!(output.as_str(&mut session), Err(Stop::Refused)));
    assert!(matches!(session.finish(Ok(())), Err(Error::State)));
}

#[test]
fn semantic_offset_error_requires_its_original_diagnostic_item() {
    let Ok(length) = usize::try_from(u64::from(u32::MAX) + 1) else {
        return;
    };
    let mut account = Account {
        requests: vec![document::Request::Items(13)],
        refusal: Some(1),
    };
    let mut session = session::Session::new(&mut account);
    assert!(matches!(offset(length, &mut session), Err(Stop::Refused)));
    assert!(matches!(offset(0, &mut session), Err(Stop::Refused)));
    assert!(matches!(session.finish(Ok(())), Err(Error::Admission(1))));
    assert_eq!(
        account.requests,
        [document::Request::Items(13), document::Request::Items(1)]
    );
}

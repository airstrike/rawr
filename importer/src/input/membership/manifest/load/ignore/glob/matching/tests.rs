use super::*;
use crate::input::membership::manifest::{reference::Account, Stop};
use rawr_toml::document::Request;

#[derive(Default)]
struct Count {
    requests: Vec<Request>,
    refuse: Option<usize>,
}
impl Admission for Count {
    type Error = usize;
    fn admit(&mut self, request: Request) -> Result<(), usize> {
        let index = self.requests.len();
        self.requests.push(request);
        if self.refuse == Some(index) {
            Err(index)
        } else {
            Ok(())
        }
    }
}

fn text(pattern: &str, session: &mut Session<'_, Account>) -> String {
    let built = super::super::build(pattern, session).unwrap().ok().unwrap();
    // GlobSet's builder actually owns this copy of the emitted regex string.
    formatted(
        format_args!("{}", std::str::from_utf8(built.regex()).unwrap()),
        session,
    )
    .unwrap()
}

fn set() -> set::Strategy {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut builder = set::Builder::new();
    builder
        .add(9, text("ab", &mut session), &mut session)
        .unwrap();
    builder
        .add(3, text("a?", &mut session), &mut session)
        .unwrap();
    builder.build(&mut session).unwrap().ok().unwrap()
}

fn output() -> Buffer<usize> {
    let mut result = Buffer::new();
    result
        .reserve_more(8, &mut Session::new(&mut Account))
        .unwrap();
    result.push(99, &mut Session::new(&mut Account)).unwrap();
    result
}

fn allocations(count: &Count) -> usize {
    count
        .requests
        .iter()
        .filter(|r| matches!(r, Request::Allocation { .. }))
        .count()
}

#[test]
fn required_extensions_keep_group_order_distinct_patterns_and_existing_output() {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut source = table::Regexes::new();
    for (index, ext, glob) in [
        (7, ".rs", "b*.rs"),
        (2, ".rs", "*.rs"),
        (8, ".txt", "b*.txt"),
    ] {
        let ext = formatted(format_args!("{ext}"), &mut session).unwrap();
        source
            .add(index, ext, text(glob, &mut session), &mut session)
            .unwrap();
    }
    let mut strategy = required::Strategy::build(source, &mut session)
        .unwrap()
        .ok()
        .unwrap();
    let mut matches = output();
    assert!(strategy
        .is_match(b"bfile.rs", b".rs", &mut session)
        .unwrap());
    strategy
        .matches_into(b"bfile.rs", b".rs", &mut matches, &mut session)
        .unwrap();
    strategy
        .matches_into(b"other.rs", b".rs", &mut matches, &mut session)
        .unwrap();
    strategy
        .matches_into(b"bfile.txt", b".txt", &mut matches, &mut session)
        .unwrap();
    assert_eq!(matches.as_slice(), [99, 7, 2, 2, 8]);
    let mut skipped = Count::default();
    assert!(!strategy
        .is_match(b"bfile.rs", b"", &mut Session::new(&mut skipped))
        .unwrap());
    assert_eq!(skipped.requests, [Request::Items(1)]);
    assert!(!strategy
        .is_match(b"x/bfile.rs", b".rs", &mut session)
        .unwrap());
    assert!(!strategy.is_match(b"bfile.c", b".c", &mut session).unwrap());
}

#[test]
fn regex_set_acquires_before_impossible_and_clears_retained_ids_before_mapping() {
    let mut strategy = set();
    let mut matches = output();
    // Core remains lazy on this impossible query, but PatternSet must allocate.
    let mut cold = Count::default();
    strategy
        .matches_into(b"", &mut matches, &mut Session::new(&mut cold))
        .unwrap();
    assert_eq!(allocations(&cold), 1);
    assert_eq!(matches.as_slice(), [99]);
    let mut warm = Count::default();
    strategy
        .matches_into(b"", &mut matches, &mut Session::new(&mut warm))
        .unwrap();
    assert_eq!(allocations(&warm), 0);
    let mut first_core = Count::default();
    assert!(strategy
        .is_match(b"ab", &mut Session::new(&mut first_core))
        .unwrap());
    assert!(allocations(&first_core) > 0);
    strategy
        .matches_into(b"ab", &mut matches, &mut Session::new(&mut Account))
        .unwrap();
    strategy
        .matches_into(b"ac", &mut matches, &mut Session::new(&mut Account))
        .unwrap();
    assert_eq!(matches.as_slice(), [99, 9, 3, 3]);
    // Pattern IDs are traversed in ascending local order, not sorted global IDs.
    let mut repeat = Count::default();
    strategy
        .matches_into(b"ac", &mut matches, &mut Session::new(&mut repeat))
        .unwrap();
    assert_eq!(allocations(&repeat), 0);
    assert_eq!(matches.as_slice(), [99, 9, 3, 3, 3]);
}

#[test]
fn semantic_compile_failure_retains_single_context_and_stops_before_later_patterns() {
    let bad = format!("(?-u)^{}a{}$", "(?:".repeat(251), ")".repeat(251));
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut source = table::Regexes::new();
    source
        .add(0, ".rs".into(), bad.clone(), &mut session)
        .unwrap();
    source
        .add(1, ".rs".into(), "unanchored".into(), &mut session)
        .unwrap();
    let Err(failure) = required::Strategy::build(source, &mut session).unwrap() else {
        panic!("expected semantic nesting failure")
    };
    assert_eq!(failure.pattern.as_deref(), Some(bad.as_str()));
    assert!(failure
        .message
        .starts_with("regex parse error in pattern 0 at bytes "));
    assert!(failure.message.ends_with("nesting limit 250 exceeded"));
    session.check().unwrap();

    let mut builder = set::Builder::new();
    builder.add(0, bad, &mut session).unwrap();
    builder.add(1, "unanchored".into(), &mut session).unwrap();
    let Err(failure) = builder.build(&mut session).unwrap() else {
        panic!("expected semantic nesting failure")
    };
    assert!(failure.pattern.is_none());
    assert!(failure
        .message
        .starts_with("regex parse error in pattern 0 at bytes "));
    assert!(failure.message.ends_with("nesting limit 250 exceeded"));
    session.check().unwrap();
}

#[test]
fn overlap_refusal_keeps_reached_output_and_blocks_clear_retry_or_boolean_fallback() {
    let mut baseline = set();
    let mut matches = output();
    let mut complete = Count::default();
    baseline
        .matches_into(b"ab", &mut matches, &mut Session::new(&mut complete))
        .unwrap();
    assert_eq!(matches.as_slice(), [99, 9, 3]);
    let mut saw_partial_append = false;
    for refuse in 0..complete.requests.len() {
        let mut strategy = set();
        let mut matches = output();
        let mut count = Count {
            refuse: Some(refuse),
            ..Count::default()
        };
        let mut session = Session::new(&mut count);
        assert!(matches!(
            strategy.matches_into(b"ab", &mut matches, &mut session),
            Err(Stop::Refused)
        ));
        let before = matches.as_slice().to_vec();
        saw_partial_append |= before == [99, 9];
        assert!(matches!(
            strategy.matches_into(b"", &mut matches, &mut session),
            Err(Stop::Refused)
        ));
        assert!(matches!(
            strategy.is_match(b"ab", &mut session),
            Err(Stop::Refused)
        ));
        assert_eq!(matches.as_slice(), before);
        assert!(
            matches!(session.finish::<()>(Err(Stop::Refused)), Err(Error::Admission(cause)) if cause == refuse)
        );
        assert_eq!(count.requests, complete.requests[..=refuse]);
    }
    assert!(saw_partial_append);
}

use rawr_toml::document::Request;

use super::super::tests::Count;
use super::{Builder, Prefilter};
use crate::input::membership::manifest::{reference::Account, session::Session, Error, Stop};

fn build(patterns: &[&[u8]]) -> Option<Prefilter> {
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut builder = Builder::new(&mut session).unwrap();
    for pattern in patterns {
        builder.add(pattern, &mut session).unwrap();
    }
    builder.build(&mut session).unwrap()
}

#[test]
fn one_pattern_owns_the_memmem_needle_and_empty_pattern_disables_retained_candidates() {
    let patterns: &[&[u8]] = &[b"needle"];
    let Prefilter::Memmem(finder) = build(patterns).unwrap() else {
        panic!("expected memmem");
    };
    assert_eq!(finder.needle(), b"needle");
    assert_eq!(finder.find(b"a needle here"), Some(2));
    let mut account = Account;
    let mut session = Session::new(&mut account);
    let mut builder = Builder::new(&mut session).unwrap();
    builder.add(b"needle", &mut session).unwrap();
    builder.add(b"", &mut session).unwrap();
    builder.add(b"later", &mut session).unwrap();
    assert_eq!(builder.one.as_ref().unwrap().slice(), b"needle");
    assert_eq!(builder.count, 1);
    assert!(builder.build(&mut session).unwrap().is_none());
}

#[test]
fn start_and_rare_selection_preserves_high_bytes_offsets_and_inert_cutoffs() {
    let Prefilter::Start1(start) = build(&[b"ax", b"ay"]).unwrap() else {
        panic!("expected start");
    };
    assert_eq!(start.byte, b'a');
    let Prefilter::Rare1(rare) = build(&[b"e!", b"e!!"]).unwrap() else {
        panic!("expected rare");
    };
    assert_eq!((rare.byte, rare.offset), (b'!', 2));
    let Prefilter::Rare1(rare) = build(&[b"\xFF\x88a", b"\xFE\x88b"]).unwrap() else {
        panic!("expected common high byte");
    };
    assert_eq!((rare.byte, rare.offset), (0x88, 1));
    let long = [b'x'; 256];
    assert!(build(&[&long, b"a", b"b", b"c"]).is_none());
}

#[test]
fn discarded_rare_candidate_is_allocated_and_its_refusal_cannot_select_start() {
    let mut builder = Builder::new(&mut Session::new(&mut Account)).unwrap();
    for pattern in [b"ax", b"ay"] {
        builder
            .add(pattern, &mut Session::new(&mut Account))
            .unwrap();
    }
    let mut count = Count::default();
    assert!(matches!(
        builder.build(&mut Session::new(&mut count)).unwrap(),
        Some(Prefilter::Start1(_))
    ));
    let allocations: Vec<_> = count
        .requests
        .iter()
        .enumerate()
        .filter_map(|(index, request)| {
            matches!(request, Request::Allocation { .. }).then_some(index)
        })
        .collect();
    assert_eq!(allocations.len(), 2);
    let index = allocations[1];
    let mut refused = Count {
        refuse: Some(index),
        ..Count::default()
    };
    let mut session = Session::new(&mut refused);
    assert!(matches!(builder.build(&mut session), Err(Stop::Refused)));
    assert!(matches!(builder.build(&mut session), Err(Stop::Refused)));
    assert!(matches!(session.finish(Ok(())), Err(Error::Admission(at)) if at == index));
    assert_eq!(refused.requests, count.requests[..index]);
}

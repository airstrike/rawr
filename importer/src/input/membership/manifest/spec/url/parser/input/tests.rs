use rawr_toml::document;

use super::Input;
use crate::input::membership::manifest::{session, Error, Stop};
use crate::input::membership::Budget;
use crate::{input, node};

mod reference;

#[derive(Default)]
struct Account {
    requests: Vec<document::Request>,
    refuse: bool,
}

impl document::Admission for Account {
    type Error = ();

    fn admit(&mut self, request: document::Request) -> Result<(), ()> {
        self.requests.push(request);
        if self.refuse {
            Err(())
        } else {
            Ok(())
        }
    }
}

#[test]
fn filtered_raw_and_utf8_cursors_match_the_exact_pinned_input() {
    let mut sources = vec![
        String::new(),
        " \0\t\r\nx\t/\n/\ré終\r\n ".into(),
        "\u{a0}\u{85}é\t💩\r\n終\u{a0}".into(),
        "x\t\n\r".into(),
        "é\t\n\r/終".into(),
    ];
    for byte in 0..=127 {
        let character = char::from(byte);
        sources.push(format!("{character}x{character}é{character}"));
    }
    for source in sources {
        let mut account = Account::default();
        let mut session = session::Session::new(&mut account);
        let input = Input::new(&source, &mut session).unwrap();
        for raw in [false, true] {
            let mut actual = input.clone();
            let mut expected = reference::input(&source);
            loop {
                assert_eq!(
                    actual.remaining(),
                    reference::remaining(&expected),
                    "{source:?}"
                );
                assert_eq!(actual.size_hint(), expected.size_hint());
                assert_eq!(actual.is_empty(), expected.is_empty());
                let left = if raw {
                    actual.next_raw()
                } else {
                    actual.next()
                };
                let right = if raw {
                    reference::raw(&mut expected)
                } else {
                    expected.next()
                };
                assert_eq!(left, right, "{source:?}");
                if left.is_none() {
                    break;
                }
            }
        }
        let mut actual = input;
        let mut expected = reference::input(&source);
        loop {
            let left = actual.next_utf8();
            let right = reference::utf8(&mut expected);
            assert_eq!(left, right, "{source:?}");
            if let (Some((_, left)), Some((_, right))) = (left, right) {
                assert!(std::ptr::eq(left, right));
            } else {
                break;
            }
        }
        session.finish(Ok(())).unwrap();
        assert_eq!(account.requests, [document::Request::String(source.len())]);
    }
}

#[test]
fn prefix_count_peek_and_rewind_keep_the_same_raw_remaining_slice() {
    for source in ["/\t/\r\\\néx", "a\tb\nc?", "x\t\n\ré", "終é\t/", "\t\n", ""] {
        let mut account = Account::default();
        let mut session = session::Session::new(&mut account);
        let input = Input::new(source, &mut session).unwrap();
        let expected = reference::input(source);
        for prefix in ["", "/", "//", "/\\", "abc", "xé", "終é", "x\t"] {
            assert_eq!(input.starts_with(prefix), expected.starts_with(prefix));
            assert_eq!(
                input.split_prefix(prefix).map(|value| value.remaining()),
                expected
                    .split_prefix(prefix)
                    .map(|value| reference::remaining(&value)),
                "{source:?}, {prefix:?}"
            );
        }
        let (count, remaining) = input.count_matching(|c| matches!(c, '/' | '\\'));
        let (expected_count, expected_remaining) = reference::count(&expected);
        assert_eq!(count, expected_count as usize);
        assert_eq!(
            remaining.remaining(),
            reference::remaining(&expected_remaining)
        );
        let (first, remaining) = input.split_first();
        let (expected_first, expected_remaining) = reference::first(&expected);
        assert_eq!(first, expected_first);
        assert_eq!(
            remaining.remaining(),
            reference::remaining(&expected_remaining)
        );
        assert_eq!(input.remaining(), reference::remaining(&expected));
        session.finish(Ok(())).unwrap();
    }
}

#[test]
fn original_spelling_is_measured_before_trim_and_admission_refusal_is_terminal() {
    let graph = node::GraphBudget::new();
    let mut allocation = input::Allocation::with_limits(13, 31, 1);
    allocation.items(13).unwrap();
    allocation.bytes(31).unwrap();
    let prior = allocation.usage;
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut session = session::Session::new(&mut budget);
    // The untrimmed four-byte spelling is measured against the one-byte
    // string target. The crossing is recorded on the original account; the
    // borrowed cursor is still handed out and spends nothing.
    let input = Input::new("  x ", &mut session).unwrap();
    assert_eq!(input.collect::<String>(), "x");
    assert!(Input::new("", &mut session).is_ok());
    session.finish(Ok(())).unwrap();
    assert_eq!(allocation.usage, prior);
    assert!(allocation.check().is_ok(), "accounting targets never latch");
    let exceeded = allocation.exceedances();
    let mut axes = exceeded.axes();
    let (Some(axis), None) = (axes.next(), axes.next()) else {
        panic!("exactly one exceeded axis: {exceeded:?}")
    };
    assert_eq!(axis.name, "membership decoded TOML string bytes");
    assert_eq!(axis.target, 1);
    assert_eq!(axis.first_requested, 4);
    assert_eq!(axis.high_water, 4);

    // An injected admission refusal is a genuine cause and stays terminal.

    let mut account = Account {
        requests: vec![document::Request::Items(7)],
        refuse: true,
    };
    let mut session = session::Session::new(&mut account);
    assert!(matches!(Input::new("x", &mut session), Err(Stop::Refused)));
    assert!(matches!(
        Input::new("next", &mut session),
        Err(Stop::Refused)
    ));
    assert!(matches!(session.finish(Ok(())), Err(Error::Admission(()))));
    assert_eq!(
        account.requests,
        [document::Request::Items(7), document::Request::String(1)]
    );
}

#[test]
fn cursor_borrows_without_spending_items_layouts_or_graph_work() {
    let graph = node::GraphBudget::new();
    graph.paths(node::MAX_PATH_CHECKS_PER_GRAPH).unwrap();
    graph
        .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
        .unwrap();
    graph
        .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
        .unwrap();
    for _ in 0..64 {
        graph.comparison().unwrap();
    }
    let source = " \té\n/\r終 ";
    let mut allocation = input::Allocation::with_limits(13, 31, source.len());
    allocation.items(13).unwrap();
    allocation.bytes(31).unwrap();
    let prior = allocation.usage;
    let mut budget = Budget {
        graph: &graph,
        allocation: &mut allocation,
    };
    let mut session = session::Session::new(&mut budget);
    let input = Input::new(source, &mut session).unwrap();
    assert_eq!(input.collect::<String>(), "é/終");
    session.finish(Ok(())).unwrap();
    assert_eq!(allocation.usage, prior);
}

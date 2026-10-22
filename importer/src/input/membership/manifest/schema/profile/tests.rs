use rawr_toml::document;

use super::{units, Debug, Opt};
use crate::input::membership::manifest::{session, storage, value, Error, Step, Stop, Tag};

mod reference;

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

fn parse(source: &str) -> document::Document<'_> {
    document::parse(source, 80, &mut Account::default()).unwrap()
}

fn input<'d>(document: &'d document::Document<'d>) -> value::Input<'d, 'd> {
    value::Input::raw(document.table().get("value").unwrap())
}

#[derive(Clone, Copy, Debug)]
enum Leaf {
    Opt,
    Debug,
    Units,
}

fn discard<A: document::Admission>(
    leaf: Leaf,
    input: value::Input<'_, '_>,
    session: &mut session::Session<'_, A>,
) -> Step<()> {
    match leaf {
        Leaf::Opt => Opt::interpret(input, session).map(|_| ()),
        Leaf::Debug => Debug::interpret(input, session).map(|_| ()),
        Leaf::Units => units(input, session).map(|_| ()),
    }
}

#[test]
fn optimization_preserves_all_signed_values_and_only_exact_string_variants() {
    for (literal, expected) in [
        ("0", "0"),
        ("-0", "0"),
        ("+2", "2"),
        ("-7", "-7"),
        ("0xff", "255"),
        ("1_000", "1000"),
        ("-9223372036854775808", "-9223372036854775808"),
        ("9223372036854775807", "9223372036854775807"),
        ("'s'", "s"),
        ("'z'", "z"),
    ] {
        let source = format!("value={literal}");
        let document = parse(&source);
        let mut account = Account::default();
        let mut session = session::Session::new(&mut account);
        let result = Opt::interpret(input(&document), &mut session);
        assert_eq!(
            session.finish(result).unwrap().as_str(),
            expected,
            "{literal}"
        );
        assert_eq!(
            account.requests,
            [document::Request::String(expected.len())]
        );
    }
}

#[test]
fn inline_decimal_matches_standard_display_across_signs_and_digit_boundaries() {
    let mut values = vec![i64::MIN, i64::MIN + 1, i64::MAX - 1, i64::MAX];
    for power in 0..=18 {
        let magnitude = 10i64.pow(power);
        for delta in -1..=1 {
            values.push(magnitude + delta);
            values.push(-magnitude + delta);
        }
    }
    for expected in values {
        let source = format!("value={expected}");
        let document = parse(&source);
        let mut account = Account::default();
        let mut session = session::Session::new(&mut account);
        let result = Opt::interpret(input(&document), &mut session);
        assert_eq!(
            session.finish(result).unwrap().as_str(),
            expected.to_string()
        );
    }
}

#[test]
fn debug_aliases_collapse_to_the_exact_pinned_enum() {
    for (literal, expected) in [
        ("false", Debug::None),
        ("true", Debug::Full),
        ("0", Debug::None),
        ("-0", Debug::None),
        ("1", Debug::Limited),
        ("2", Debug::Full),
        ("'none'", Debug::None),
        ("'limited'", Debug::Limited),
        ("'full'", Debug::Full),
        ("'line-directives-only'", Debug::LineDirectivesOnly),
        ("'line-tables-only'", Debug::LineTablesOnly),
    ] {
        let source = format!("value={literal}");
        let document = parse(&source);
        let mut account = Account::default();
        let mut session = session::Session::new(&mut account);
        let result = Debug::interpret(input(&document), &mut session);
        assert_eq!(session.finish(result).unwrap(), expected, "{literal}");
    }
    assert!(Debug::None < Debug::LineDirectivesOnly);
    assert!(Debug::LineDirectivesOnly < Debug::LineTablesOnly);
    assert!(Debug::LineTablesOnly < Debug::Limited);
    assert!(Debug::Limited < Debug::Full);
}

#[test]
fn codegen_units_keeps_zero_and_the_complete_u32_domain() {
    for (literal, expected) in [
        ("0", 0),
        ("-0", 0),
        ("+1", 1),
        ("4294967295", u32::MAX),
        ("0xffffffff", u32::MAX),
    ] {
        let source = format!("value={literal}");
        let document = parse(&source);
        let mut account = Account::default();
        let mut session = session::Session::new(&mut account);
        let result = units(input(&document), &mut session);
        assert_eq!(session.finish(result).unwrap(), expected);
        assert!(account.requests.is_empty());
    }
}

#[test]
fn leaf_rejections_preserve_dispatch_category_and_source_span() {
    for (leaf, literal, tag) in [
        (Leaf::Opt, "'3'", Tag::Variant),
        (Leaf::Opt, "'S'", Tag::Variant),
        (Leaf::Opt, "'s '", Tag::Variant),
        (Leaf::Opt, "true", Tag::Type),
        (Leaf::Opt, "1.0", Tag::Type),
        (Leaf::Opt, "9223372036854775808", Tag::Type),
        (Leaf::Opt, "-9223372036854775809", Tag::Type),
        (Leaf::Debug, "3", Tag::Variant),
        (Leaf::Debug, "-1", Tag::Variant),
        (Leaf::Debug, "'1'", Tag::Variant),
        (Leaf::Debug, "'Full'", Tag::Variant),
        (Leaf::Debug, "'full '", Tag::Variant),
        (Leaf::Debug, "1.0", Tag::Type),
        (Leaf::Debug, "9223372036854775808", Tag::Type),
        (Leaf::Units, "-1", Tag::Variant),
        (Leaf::Units, "4294967296", Tag::Variant),
        (Leaf::Units, "18446744073709551615", Tag::Variant),
        (Leaf::Units, "18446744073709551616", Tag::Type),
        (Leaf::Units, "'1'", Tag::Type),
        (Leaf::Units, "true", Tag::Type),
        (Leaf::Units, "1.0", Tag::Type),
    ] {
        let source = format!("value={literal}");
        let document = parse(&source);
        let mut account = Account::default();
        let mut session = session::Session::new(&mut account);
        let result = discard(leaf, input(&document), &mut session);
        let Err(Error::Invalid(error)) = session.finish(result) else {
            panic!("{leaf:?} {literal}");
        };
        assert_eq!(error.tag, tag, "{leaf:?} {literal}");
        assert_eq!(error.span, Some((6..source.len()).into()));
    }
    for leaf in [Leaf::Opt, Leaf::Debug, Leaf::Units] {
        for (literal, tag) in [
            ("[]", Tag::Type),
            ("{}", Tag::Type),
            ("2026-09-05", Tag::Type),
            ("1e9999", Tag::FloatOverflow),
            (
                "340282366920938463463374607431768211456",
                Tag::IntegerOverflow,
            ),
        ] {
            let source = format!("value={literal}");
            let document = parse(&source);
            let mut account = Account::default();
            let mut session = session::Session::new(&mut account);
            let result = discard(leaf, input(&document), &mut session);
            assert!(
                matches!(session.finish(result), Err(Error::Invalid(error)) if error.tag == tag)
            );
        }
    }
}

#[test]
fn untagged_integer_dispatch_and_plain_u32_visitor_remain_distinct() {
    for kind in [
        value::Kind::U64(1),
        value::Kind::I128(1),
        value::Kind::U128(1),
    ] {
        let mut account = Account::default();
        let mut session = session::Session::new(&mut account);
        let mut projection = value::Projection::new(value::Mode::Direct);
        projection
            .append(value::Node { kind, span: None }, None, &mut session)
            .unwrap();
        let input = value::Input::replay(&projection, 0);
        assert_eq!(Opt::interpret(input, &mut session).unwrap().as_str(), "1");
        assert_eq!(
            Debug::interpret(input, &mut session).unwrap(),
            Debug::Limited
        );
        let result = units(input, &mut session);
        if matches!(kind, value::Kind::U64(_)) {
            assert_eq!(result.unwrap(), 1);
        } else {
            assert!(
                matches!(result, Err(Stop::Rejected(error)) if error.tag == Tag::Type && error.span.is_none())
            );
        }
        session.finish(Ok(())).unwrap();
    }
}

#[test]
fn every_string_and_diagnostic_refusal_is_terminal_for_later_fixed_scalars() {
    for (leaf, literal) in [
        (Leaf::Opt, "-9223372036854775808"),
        (Leaf::Opt, "'bad'"),
        (Leaf::Debug, "'bad'"),
        (Leaf::Units, "-1"),
    ] {
        let source = format!("value={literal}");
        let document = parse(&source);
        let followup = parse("value=false");
        let mut full = Account::default();
        let mut session = session::Session::new(&mut full);
        let result = discard(leaf, input(&document), &mut session);
        let _ = session.finish(result);
        for refusal in 0..full.requests.len() {
            let mut account = Account {
                refusal: Some(refusal),
                ..Account::default()
            };
            let mut session = session::Session::new(&mut account);
            assert!(matches!(
                discard(leaf, input(&document), &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(
                Debug::interpret(input(&followup), &mut session),
                Err(Stop::Refused)
            ));
            assert!(
                matches!(session.finish(Ok(())), Err(Error::Admission(index)) if index == refusal)
            );
            assert_eq!(account.requests, full.requests[..=refusal]);
        }
    }
}

#[test]
fn original_account_charges_the_retained_inline_slot_and_exact_string_floor() {
    use crate::input;
    use crate::input::membership::manifest::tests::one_short;
    use crate::input::membership::Budget;
    use crate::node;

    let document = parse("value=-9223372036854775808");
    let graph = node::GraphBudget::new();
    graph.paths(node::MAX_PATH_CHECKS_PER_GRAPH).unwrap();
    graph
        .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
        .unwrap();
    graph
        .decode(node::MAX_DECODED_BYTES_PER_GRAPH as usize)
        .unwrap();
    let bytes = 19 + 4 * std::mem::size_of::<Opt>();
    // One short of an item, byte, or string target still admits the
    // retained slot and records only that axis at the exact total. The string
    // row is the 20-byte literal itself, so its first crossing is its total.
    const STRING_RESOURCE: &str = "membership decoded TOML string bytes";
    for (items, limit, string, exceeded) in [
        (8, bytes, 20, None),
        (7, bytes, 20, Some((input::INPUT_ITEMS_RESOURCE, 7, 8))),
        (
            8,
            bytes - 1,
            20,
            Some((input::INPUT_BYTES_RESOURCE, bytes - 1, bytes)),
        ),
        (8, bytes, 19, Some((STRING_RESOURCE, 19, 20))),
    ] {
        let mut allocation = input::Allocation::with_limits(items, limit, string);
        allocation.items(7).unwrap();
        allocation.bytes(19).unwrap();
        let mut budget = Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        let mut session = session::Session::new(&mut budget);
        let mut retained = storage::Buffer::new();
        let result = (|| {
            let converted = Opt::interpret(input(&document), &mut session)?;
            retained.push(converted, &mut session)?;
            Ok(())
        })();
        assert!(
            session.finish(result).is_ok(),
            "an accounting target is not an admission gate"
        );
        assert_eq!(allocation.usage.items, 8);
        assert_eq!(allocation.usage.bytes, bytes);
        assert_eq!(retained.as_slice()[0].as_str(), "-9223372036854775808");
        assert!(allocation.check().is_ok(), "accounting targets never latch");
        let record = allocation.exceedances();
        match exceeded {
            Some((resource, target, total)) => {
                assert!(one_short(&record, resource, total), "{record:?}");
                let axis = record.axis(resource).unwrap();
                assert_eq!(axis.target, target as u64, "{resource}");
                if resource == STRING_RESOURCE {
                    assert_eq!(axis.first_requested, total as u64);
                }
            }
            None => assert!(!record.is_exceeded(), "{record:?}"),
        }
    }
}

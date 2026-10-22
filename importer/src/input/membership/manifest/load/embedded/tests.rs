use super::*;
use crate::input::membership::manifest::{Source, Stop};

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
fn message(kind: Kind, raw: &str, primary: Span) -> String {
    match kind {
        Kind::ShortOpening(n) => format!("found {n} `-` in rust frontmatter, expected at least 3"),
        Kind::LongOpening(n) => format!("too many `-` symbols: frontmatter openings may be delimited by up to 255 `-` symbols, but found {n}"),
        Kind::Unclosed => {
            let start = raw.find("---").unwrap();
            let length = dashes(&raw[start..]);
            format!("unclosed frontmatter; expected `{}`", "-".repeat(length))
        }
        Kind::Fewer(n) => format!("closing code fence has {n} less `-` than the opening fence"),
        Kind::More(n) => format!("closing code fence has {n} more `-` than the opening fence"),
        Kind::Trailing => "unexpected characters after frontmatter close".into(),
        Kind::Multiple => "only one frontmatter is supported".into(),
        Kind::Attributes => format!("unsupported frontmatter infostring attributes: `{}`", raw[primary.start..primary.end].strip_prefix("cargo,").unwrap()),
        Kind::Info => format!("unsupported frontmatter infostring `{}`; specify `cargo` for embedding a manifest", &raw[primary.start..primary.end]),
    }
}

#[test]
fn complete_expansion_matches_actual_pinned_source_observations() {
    for line in include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/reference/manifest/frontmatter-panic/expansion.jsonl"
    ))
    .lines()
    {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let raw = row["source"].as_str().unwrap();
        let mut account = Account::default();
        let mut session = Session::new(&mut account);
        let result = expand(raw, &mut session);
        match session.finish(result) {
            Ok(output) => assert_eq!(row["output"].as_str(), Some(output.as_str()), "{raw:?}"),
            Err(Error::Invalid(Diagnostic {
                tag: Tag::Frontmatter(error),
                span: Some(primary),
                ..
            })) => {
                assert_eq!(
                    row["error"].as_str(),
                    Some(message(error.kind, raw, primary).as_str()),
                    "{raw:?}"
                );
                assert_eq!(
                    row["primary"],
                    serde_json::json!({"start":primary.start,"end":primary.end}),
                    "{raw:?}"
                );
                let visible: Vec<_> = error
                    .visible
                    .iter()
                    .flatten()
                    .map(|span| serde_json::json!({"start":span.start,"end":span.end}))
                    .collect();
                assert_eq!(row["visible"], serde_json::json!(visible), "{raw:?}");
            }
            Err(Error::Source {
                cause: Source::FrontmatterPartialAdvance { .. },
                span: Some(cursor),
            }) => {
                assert_eq!(row["panic"], true, "{raw:?}");
                assert_eq!(cursor.start, cursor.end);
                assert!(raw.is_char_boundary(cursor.start));
            }
            other => panic!("unexpected {other:?} for {raw:?}"),
        }
    }
}

#[test]
fn failure_retention_and_output_allocation_use_one_terminal_account() {
    for raw in [
        "---\n-",
        "---\n-🦀",
        "---cargo\n[workspace]\n---\nfn main() {}",
        "---wrong\n---",
    ] {
        let mut baseline = Account::default();
        let mut session = Session::new(&mut baseline);
        let _ = expand(raw, &mut session);
        for refusal in 0..baseline.requests.len() {
            let mut account = Account {
                refusal: Some(refusal),
                ..Account::default()
            };
            let mut session = Session::new(&mut account);
            storage::ALLOCATIONS.with(|count| count.set(0));
            assert!(matches!(expand(raw, &mut session), Err(Stop::Refused)));
            let allocations = storage::ALLOCATIONS.with(|count| count.get());
            assert!(matches!(
                expand("---\n---", &mut session),
                Err(Stop::Refused)
            ));
            assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), allocations);
            assert!(
                matches!(session.finish(Ok(())), Err(Error::Admission(index)) if index == refusal)
            );
            assert_eq!(account.requests, baseline.requests[..=refusal]);
        }
    }
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    storage::ALLOCATIONS.with(|count| count.set(0));
    assert!(expand("---\n-", &mut session).is_err());
    assert!(expand("valid Rust", &mut session).is_err());
    assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), 0);
    assert!(matches!(
        session.finish(Ok(())),
        Err(Error::Source {
            cause: Source::FrontmatterPartialAdvance { bytes: 2 },
            ..
        })
    ));
}

#[test]
fn expansion_uses_exact_original_limits_and_empty_output_needs_no_allocation() {
    use crate::input::membership::manifest::tests::admits_one_short;
    use crate::input::membership::Budget;
    use crate::{input, node};
    let raw = "#!/bin/cargo\n---cargo\n[workspace]\n---\nfn main() {}";
    let run = |items, bytes, string| {
        let graph = node::GraphBudget::new();
        let mut allocation = input::Allocation::with_limits(items, bytes, string);
        allocation.items(7).unwrap();
        allocation.bytes(31).unwrap();
        let mut budget = Budget {
            graph: &graph,
            allocation: &mut allocation,
        };
        let mut session = Session::new(&mut budget);
        let result = expand(raw, &mut session);
        let result = session.finish(result);
        assert!(allocation.check().is_ok(), "accounting targets never latch");
        (result.is_ok(), allocation.usage(), allocation.exceedances())
    };
    let (ok, usage, _) = run(usize::MAX, usize::MAX, usize::MAX);
    assert!(ok);
    // Whole-document ownership is independent of the decoded-scalar cap.
    assert_eq!(
        run(usage.items, usage.bytes, 0),
        (true, usage, crate::exceedance::Record::new())
    );
    // One short of an aggregate target admits and records that axis.
    admits_one_short(
        run(usage.items - 1, usage.bytes, 0),
        usage,
        input::INPUT_ITEMS_RESOURCE,
        usage.items,
    );
    admits_one_short(
        run(usage.items, usage.bytes - 1, 0),
        usage,
        input::INPUT_BYTES_RESOURCE,
        usage.bytes,
    );
    let mut account = Account::default();
    let mut session = Session::new(&mut account);
    storage::ALLOCATIONS.with(|count| count.set(0));
    assert_eq!(expand("fn main() {}", &mut session).unwrap(), "");
    assert_eq!(storage::ALLOCATIONS.with(|count| count.get()), 0);
    session.finish(Ok(())).unwrap();
    assert!(!account
        .requests
        .iter()
        .any(|request| matches!(request, document::Request::Allocation { .. })));
}

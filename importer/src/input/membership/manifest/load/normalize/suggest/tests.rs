use super::*;
use crate::input::membership::manifest::reference;

#[allow(clippy::len_zero, reason = "unchanged pinned upstream source")]
#[path = "reference.rs"]
mod pinned;

fn lower(text: &str) -> String {
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    lowercase(text, &mut session)
        .unwrap()
        .as_slice()
        .iter()
        .collect()
}

#[test]
fn contextual_sigma_matches_pinned_str_lowercase_at_property_boundaries() {
    for text in [
        "Σ",
        "AΣ",
        "ΑΣΑ",
        "ΟΔΥΣΣΕΎΣ",
        "AΣ\u{301}",
        "AΣ\u{301}A",
        "A'Σ",
        "AΣ'B",
        "İΣ",
        "ǅΣ",
        "a.Σ:",
        "\0Σ",
    ] {
        assert_eq!(lower(text), text.to_lowercase(), "{text:?}");
    }
    for &(start, end) in properties::CASE_IGNORABLE.iter().chain(properties::LT) {
        for point in [start.saturating_sub(1), start, end - 1, end] {
            if let Some(ch) = char::from_u32(point) {
                for text in [format!("A{ch}Σ"), format!("AΣ{ch}A"), format!("{ch}Σ")] {
                    assert_eq!(lower(&text), text.to_lowercase(), "U+{point:04X}: {text:?}");
                }
            }
        }
    }
}

#[test]
fn distance_matches_unchanged_pinned_reference() {
    let names = [
        "",
        "a",
        "ab",
        "ba",
        "CA",
        "ABC",
        "CHECK",
        "check",
        "AΣ",
        "aς",
        "aσ",
        "AΣA",
        "aσa",
        "İ",
        "i\u{307}",
        "é",
        "e\u{301}",
        "prefix-cat-suffix",
        "prefix-cut-suffix",
        "longer than the limit",
    ];
    for a in names {
        for b in names {
            for limit in 0..=3 {
                let mut account = reference::Account;
                let mut session = Session::new(&mut account);
                assert_eq!(
                    distance(a, b, limit, &mut session).unwrap(),
                    pinned::edit_distance(a, b, limit),
                    "{a:?} {b:?} limit {limit}"
                );
            }
        }
    }
}

#[test]
fn closest_retains_first_tie_original_spelling_and_limit_three() {
    let names = ["cat", "cut", "CAT", "distant"];
    for choice in ["cot", "CAT", "unrelated"] {
        let mut account = reference::Account;
        let mut session = Session::new(&mut account);
        assert_eq!(
            select(choice, names.into_iter(), &mut session).unwrap(),
            pinned::closest(choice, names.into_iter(), |name| *name)
        );
    }
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    assert_eq!(
        select("cot", names.into_iter(), &mut session).unwrap(),
        Some("cat")
    );
    assert_eq!(
        select("anything", std::iter::empty(), &mut session).unwrap(),
        None
    );
}

#[derive(Default)]
struct Account {
    requests: Vec<document::Request>,
    refusal: Option<usize>,
    caps: Option<[usize; 3]>,
    used: [usize; 3],
}
impl document::Admission for Account {
    type Error = &'static str;
    fn admit(&mut self, request: document::Request) -> Result<(), Self::Error> {
        let index = self.requests.len();
        self.requests.push(request);
        match request {
            document::Request::Items(count) => self.used[0] += count,
            document::Request::String(length) => self.used[1] = self.used[1].max(length),
            document::Request::Allocation { bytes, .. } => self.used[2] += bytes,
        }
        if self.refusal == Some(index)
            || self
                .caps
                .is_some_and(|caps| self.used.iter().zip(caps).any(|(used, cap)| *used > cap))
        {
            Err("suggestion refused")
        } else {
            Ok(())
        }
    }
}
fn run(account: &mut Account) -> Result<(), Error<&'static str>> {
    let mut session = Session::new(account);
    let result = (|| {
        let result = select("AΣy", ["AΣx", "aςy", "zzz"].into_iter(), &mut session)?;
        assert_eq!(result, Some("AΣx"));
        Ok(())
    })();
    if result.is_err() {
        assert!(session.check().is_err());
        assert!(session.items(1).is_err());
    }
    session.finish(result)
}

#[test]
fn every_refusal_preserves_exact_request_prefix_and_stops() {
    let mut full = Account::default();
    run(&mut full).unwrap();
    for refusal in 0..full.requests.len() {
        let mut account = Account {
            refusal: Some(refusal),
            ..Account::default()
        };
        assert!(
            matches!(
                run(&mut account),
                Err(Error::Admission("suggestion refused"))
            ),
            "request {refusal}"
        );
        assert_eq!(account.requests, full.requests[..=refusal]);
    }
}

#[test]
fn exact_and_one_short_bounds_include_context_scans_and_dp_cells() {
    let mut full = Account::default();
    run(&mut full).unwrap();
    let mut exact = Account {
        caps: Some(full.used),
        ..Account::default()
    };
    run(&mut exact).unwrap();
    assert_eq!(exact.requests, full.requests);
    for dimension in 0..3 {
        let mut caps = full.used;
        caps[dimension] -= 1;
        let mut account = Account {
            caps: Some(caps),
            ..Account::default()
        };
        assert!(
            matches!(
                run(&mut account),
                Err(Error::Admission("suggestion refused"))
            ),
            "dimension {dimension}"
        );
    }
}

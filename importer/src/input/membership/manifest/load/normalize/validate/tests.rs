use super::*;
use crate::input::membership::manifest::{reference, Stop};

fn inspect(
    source: &str,
    name: Option<&str>,
    kind: Option<Kind>,
    edition: u16,
) -> (Result<(), Tag>, Vec<String>, Vec<String>) {
    let document = document::parse(source, 80, &mut reference::Account).unwrap();
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    let original = Target::interpret(
        value::Input::raw(document.table().get("value").unwrap()),
        &mut session,
    )
    .unwrap();
    let mut warnings = Warnings::new();
    let mut errors = Errors::new();
    let result = match kind {
        Some(kind) => common(
            Some(&original),
            name,
            kind,
            edition,
            &mut warnings,
            &mut session,
        ),
        None => binary(
            Some(&original),
            name,
            edition,
            &mut warnings,
            &mut errors,
            &mut session,
        ),
    };
    let result = match result {
        Ok(()) => Ok(()),
        Err(Stop::Rejected(diagnostic)) => Err(diagnostic.tag),
        Err(Stop::Refused) => panic!("refusal"),
    };
    (
        result,
        warnings.as_slice().to_vec(),
        errors.as_slice().to_vec(),
    )
}

#[test]
fn names_reject_before_fields_and_only_binaries_have_artifact_restrictions() {
    for (name, tag) in [
        (None, Tag::TargetMissingName),
        (Some(" \t\n"), Tag::TargetEmptyName),
        (Some("deps"), Tag::BinaryConflictingName),
        (Some("examples"), Tag::BinaryConflictingName),
        (Some("build"), Tag::BinaryConflictingName),
        (Some("incremental"), Tag::BinaryConflictingName),
    ] {
        let (result, warnings, errors) = inspect(
            "value={crate_type=['rlib'],proc_macro=true}",
            name,
            None,
            2024,
        );
        assert_eq!(result, Err(tag));
        assert!(warnings.is_empty());
        assert!(errors.is_empty());
    }
    for name in ["Deps", "with-hyphen", "name with spaces", "CON"] {
        assert_eq!(inspect("value={}", Some(name), None, 2024).0, Ok(()));
    }
    for kind in [Kind::Example, Kind::Test, Kind::Bench] {
        assert_eq!(
            inspect("value={}", Some("deps"), Some(kind), 2024).0,
            Ok(())
        );
    }
}

#[test]
fn binary_reports_in_source_order_and_skips_alias_rejection_even_in_2024() {
    let (result, warnings, errors) = inspect(
        "value={crate_type=['bin','rlib'],proc_macro=true}",
        Some("app"),
        None,
        2024,
    );
    assert_eq!(result, Ok(()));
    assert!(warnings.is_empty());
    assert_eq!(errors, ["the target `app` is a binary and can't have any crate-types set (currently \"bin, rlib\")", "the target `app` is a binary and can't have `proc-macro` set `true`"]);
}

#[test]
fn preferred_empty_or_false_values_select_alias_checks_instead_of_old_values() {
    let (result, warnings, errors) = inspect(
        "value={crate-type=[],crate_type=['rlib'],proc-macro=false,proc_macro=true}",
        Some("app"),
        None,
        2021,
    );
    assert_eq!(result, Ok(()));
    assert!(errors.is_empty());
    assert_eq!(warnings, ["`crate_type` is redundant with `crate-type`, preferring `crate-type` in the `app` binary target", "`proc_macro` is redundant with `proc-macro`, preferring `proc-macro` in the `app` binary target"]);
    assert_eq!(
        inspect(
            "value={crate_type=[],proc_macro=false}",
            Some("app"),
            None,
            2024
        )
        .0,
        Err(Tag::TargetCrateTypeUnderscore)
    );
    assert_eq!(
        inspect("value={proc_macro=false}", Some("app"), None, 2024).0,
        Err(Tag::TargetProcMacroUnderscore)
    );
}

#[test]
fn common_checks_proc_before_crates_with_exact_kind_wording() {
    for (kind, human) in [
        (Kind::Example, "example"),
        (Kind::Test, "test"),
        (Kind::Bench, "benchmark"),
    ] {
        let (result, warnings, errors) = inspect(
            "value={crate_type=['rlib'],proc_macro=true}",
            Some("app"),
            Some(kind),
            2021,
        );
        assert_eq!(result, Ok(()));
        assert!(errors.is_empty());
        assert_eq!(warnings, [format!("`proc_macro` is deprecated in favor of `proc-macro` and will not work in the 2024 edition\n(in the `app` {human} target)"), format!("`crate_type` is deprecated in favor of `crate-type` and will not work in the 2024 edition\n(in the `app` {human} target)")]);
        assert_eq!(
            inspect(
                "value={crate_type=[],proc_macro=false}",
                Some("app"),
                Some(kind),
                2024
            )
            .0,
            Err(Tag::TargetProcMacroUnderscore)
        );
    }
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
            document::Request::Items(amount) => self.used[0] += amount,
            document::Request::String(length) => self.used[1] = self.used[1].max(length),
            document::Request::Allocation { bytes, .. } => self.used[2] += bytes,
        }
        if self.refusal == Some(index)
            || self
                .caps
                .is_some_and(|caps| self.used.iter().zip(caps).any(|(used, cap)| *used > cap))
        {
            Err("validation refused")
        } else {
            Ok(())
        }
    }
}
fn run(account: &mut Account) -> Result<(), Error<&'static str>> {
    let document = document::parse(
        "value={crate_type=['bin','rlib'],proc_macro=true}",
        80,
        &mut reference::Account,
    )
    .unwrap();
    let mut session = Session::new(account);
    let result = (|| {
        let target = Target::interpret(
            value::Input::raw(document.table().get("value").unwrap()),
            &mut session,
        )?;
        let mut warnings = Warnings::new();
        let mut errors = Errors::new();
        binary(
            Some(&target),
            Some("app"),
            2024,
            &mut warnings,
            &mut errors,
            &mut session,
        )?;
        assert_eq!(errors.as_slice().len(), 2);
        // Reach ordinary warnings on the same account after critical reports.
        common(
            Some(&target),
            Some("app"),
            Kind::Example,
            2021,
            &mut warnings,
            &mut session,
        )?;
        assert_eq!(warnings.as_slice().len(), 2);
        Ok(())
    })();
    if result.is_err() {
        assert!(session.check().is_err());
        assert!(session.items(1).is_err());
    }
    session.finish(result)
}

#[test]
fn every_admission_refusal_stops_before_further_work_or_report_retention() {
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
                Err(Error::Admission("validation refused"))
            ),
            "request {refusal}"
        );
        assert_eq!(account.requests, full.requests[..=refusal]);
    }
}

#[test]
fn exact_and_one_short_limits_share_schema_warnings_and_critical_reports() {
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
        let mut short = Account {
            caps: Some(caps),
            ..Account::default()
        };
        assert!(
            matches!(run(&mut short), Err(Error::Admission("validation refused"))),
            "dimension {dimension}"
        );
    }
}

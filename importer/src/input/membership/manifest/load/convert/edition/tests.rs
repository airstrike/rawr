use super::*;
use crate::input::membership::manifest::reference;

fn selected(edition: Option<&str>, rust: Option<&str>) -> (Result<Edition, Failure>, Vec<String>) {
    let rust = rust.map(|rust| Rust::parse(rust).unwrap());
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    let mut warnings = Warnings::new();
    let result = select(edition, rust.as_ref(), &mut warnings, &mut session);
    (
        session.finish(result).unwrap(),
        warnings.as_slice().to_vec(),
    )
}

#[test]
fn supported_editions_and_future_keep_separate_stability() {
    for text in ["2015", "2018", "2021", "2024", "future"] {
        let (result, warnings) = selected(Some(text), None);
        let edition = result.unwrap();
        assert_eq!(edition.text(), text);
        assert_eq!(edition.is_stable(), text != "future");
        assert!(warnings.is_empty());
    }
}

#[test]
fn unknown_and_future_numeric_spellings_keep_parse_context_and_exact_cause() {
    for text in ["2025", "2049", "+2025", "02025"] {
        let (result, warnings) = selected(Some(text), Some("1.85"));
        let error = result.unwrap_err();
        assert_eq!(error.context(), Some("failed to parse the `edition` key"));
        assert_eq!(error.message(), format!("this version of Cargo is older than the `{text}` edition, and only supports `2015`, `2018`, `2021`, and `2024` editions."));
        assert!(warnings.is_empty());
    }
    for text in [
        "", " 2024", "2024 ", "2023", "2050", "65536", "FUTURE", "-2025",
    ] {
        let (result, _) = selected(Some(text), None);
        assert_eq!(result.unwrap_err().message(), format!("supported edition values are `2015`, `2018`, `2021`, or `2024`, but `{text}` is unknown"));
    }
}

#[test]
fn edition_msrv_thresholds_accept_partial_patch_and_reject_other_major() {
    for (edition, minimum, before) in [("2018", 31, 30), ("2021", 56, 55), ("2024", 85, 84)] {
        for rust in [
            format!("1.{minimum}"),
            format!("1.{minimum}.0"),
            format!("1.{minimum}.9"),
            "1.999".into(),
        ] {
            assert!(selected(Some(edition), Some(&rust)).0.is_ok());
        }
        for rust in [
            "1".to_owned(),
            format!("1.{before}.999"),
            "0.999".into(),
            "2".into(),
            "2.999".into(),
        ] {
            let error = selected(Some(edition), Some(&rust)).0.unwrap_err();
            assert_eq!(error.context(), None);
            assert_eq!(error.message(), format!("rust-version {rust} is incompatible with the version (1.{minimum}.0) required by the specified edition ({edition})"));
        }
    }
    for edition in ["2015", "future"] {
        assert!(selected(Some(edition), Some("0.1")).0.is_ok());
        assert!(selected(Some(edition), Some("2")).0.is_ok());
    }
}

#[test]
fn absent_edition_always_defaults_to_2015_with_source_msrv_warning() {
    for (rust, tip) in [
        (None, Some(" while the latest is `2024`")),
        (Some("1"), None),
        (Some("1.30.999"), None),
        (Some("2.99"), None),
        (
            Some("1.31"),
            Some(" while 2018 is compatible with `rust-version`"),
        ),
        (
            Some("1.55"),
            Some(" while 2018 is compatible with `rust-version`"),
        ),
        (
            Some("1.56"),
            Some(" while 2021 is compatible with `rust-version`"),
        ),
        (
            Some("1.84"),
            Some(" while 2021 is compatible with `rust-version`"),
        ),
        (Some("1.85"), Some(" while the latest is `2024`")),
    ] {
        let (result, warnings) = selected(None, rust);
        assert_eq!(result.unwrap().text(), "2015");
        assert_eq!(
            warnings,
            tip.into_iter()
                .map(|tip| format!("`package.edition` is unspecified, defaulting to `2015`{tip}"))
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn original_admission_refusal_never_becomes_semantic_edition_failure() {
    struct Refuse;
    impl document::Admission for Refuse {
        type Error = &'static str;
        fn admit(&mut self, _: document::Request) -> Result<(), Self::Error> {
            Err("original exhausted")
        }
    }
    let mut account = Refuse;
    let mut session = Session::new(&mut account);
    let mut warnings = Warnings::new();
    let result = select(Some("unknown"), None, &mut warnings, &mut session);
    assert!(result.is_err());
    assert!(select(None, None, &mut warnings, &mut session).is_err());
    assert!(session.finish(result).is_err());
    assert!(warnings.as_slice().is_empty());
}

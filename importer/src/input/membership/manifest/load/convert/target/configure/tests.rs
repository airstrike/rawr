use super::*;
use crate::input::membership::manifest::{reference, value};

fn configured(source: &str, kind: Kind) -> (Result<Settings, edition::Failure>, Vec<String>) {
    let mut account = reference::Account;
    let document = document::parse(source, 80, &mut account).unwrap();
    let mut session = Session::new(&mut account);
    let original = document
        .table()
        .get("target")
        .map(|input| Target::interpret(value::Input::raw(input), &mut session).unwrap());
    let mut warnings = Warnings::new();
    let edition = edition::select(Some("2021"), None, &mut warnings, &mut session)
        .unwrap()
        .unwrap();
    let result = configure(
        original.as_ref(),
        "named",
        kind,
        edition,
        &mut warnings,
        &mut session,
    );
    (
        session.finish(result).unwrap(),
        warnings.as_slice().to_vec(),
    )
}

#[test]
fn ordinary_constructor_defaults_match_each_kind() {
    for (kind, test, doc, doctest, bench) in [
        (Kind::Lib, true, true, true, true),
        (Kind::Bin, true, true, false, true),
        (Kind::Example, false, false, false, false),
        (Kind::Test, true, false, false, false),
        (Kind::Bench, false, false, false, true),
    ] {
        let (result, warnings) = configured("", kind);
        let settings = result.unwrap();
        assert_eq!(
            (
                settings.tested(),
                settings.documented(),
                settings.doctested(),
                settings.benched()
            ),
            (test, doc, doctest, bench)
        );
        assert!(settings.harness());
        assert!(!settings.proc_macro() && !settings.for_host());
        assert_eq!(settings.doc_scrape_examples(), Scrape::Unset);
        assert_eq!(settings.edition().text(), "2021");
        assert!(warnings.is_empty());
    }
}

#[test]
fn explicit_false_overrides_defaults_and_preferred_alias_controls_host_flag() {
    let (result,_) = configured("target={test=false,doc=false,doctest=false,bench=false,harness=false,proc-macro=false,proc_macro=true,doc-scrape-examples=false}",Kind::Lib);
    let settings = result.unwrap();
    assert!(
        !settings.tested()
            && !settings.documented()
            && !settings.doctested()
            && !settings.benched()
            && !settings.harness()
    );
    assert!(!settings.proc_macro() && !settings.for_host());
    assert_eq!(settings.doc_scrape_examples(), Scrape::Disabled);
    let (result, _) = configured(
        "target={proc_macro=true,doc-scrape-examples=true}",
        Kind::Example,
    );
    let settings = result.unwrap();
    assert!(settings.proc_macro() && settings.for_host());
    assert_eq!(settings.doc_scrape_examples(), Scrape::Enabled);
}

#[test]
fn proc_macro_crate_type_does_not_set_boolean_or_host_flags() {
    let (result, _) = configured("target={crate-type=['proc-macro']}", Kind::Lib);
    let settings = result.unwrap();
    assert!(!settings.proc_macro() && !settings.for_host());
}

#[test]
fn target_edition_warning_precedes_parse_and_future_needs_no_gate_here() {
    let (result, warnings) = configured("target={edition='unknown'}", Kind::Bench);
    assert_eq!(
        warnings,
        ["`edition` is set on benchmark `named` which is deprecated"]
    );
    assert_eq!(
        result.unwrap_err().context(),
        Some("failed to parse the `edition` key")
    );
    let (result, warnings) = configured("target={edition='future'}", Kind::Lib);
    assert_eq!(result.unwrap().edition().text(), "future");
    assert_eq!(
        warnings,
        ["`edition` is set on library `named` which is deprecated"]
    );
}

#[test]
fn build_constructor_defaults_are_separate_from_ordinary_configure() {
    let mut account = reference::Account;
    let mut session = Session::new(&mut account);
    let mut warnings = Warnings::new();
    let edition = edition::select(Some("2024"), None, &mut warnings, &mut session)
        .unwrap()
        .unwrap();
    for kind in [Kind::CustomBuild, Kind::Metabuild] {
        let settings = defaults(kind, edition, &mut session).unwrap();
        assert!(
            !settings.tested()
                && !settings.benched()
                && !settings.documented()
                && !settings.doctested()
                && !settings.proc_macro()
        );
        assert!(settings.for_host() && settings.harness());
        assert_eq!(settings.doc_scrape_examples(), Scrape::Disabled);
        assert_eq!(
            settings.edition().text(),
            if kind == Kind::Metabuild {
                "2018"
            } else {
                "2024"
            }
        );
    }
    session.finish(Ok(())).unwrap();
}

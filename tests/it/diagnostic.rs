use rawr::diagnostic::{Error, ExternalPosition, LabelKind, Report, Source, code};
use rawr::{Engine, ParseErrorType, Position, Span};

#[test]
fn report_validates_owned_cross_source_evidence() {
    let first = Source::new("src/first.rs", "struct Café {}\n").unwrap();
    let second = Source::new("src/second.rs", "struct Café {}\n").unwrap();
    let report = Report::error(code::RESOLVE_DUPLICATE_ITEM, "duplicate item `Café`")
        .primary(second, Span::new(Position::new(1, 8), Position::new(1, 12)), Some("duplicate declaration".into()))
        .unwrap()
        .secondary(first, Span::new(Position::new(1, 8), Position::new(1, 12)), Some("first declared here".into()))
        .unwrap()
        .help("rename one declaration")
        .finish();

    assert_eq!(report.category(), rawr::diagnostic::Category::Resolve);
    assert_eq!(report.labels().len(), 2);
    assert_eq!(report.labels()[0].kind(), LabelKind::Primary);
    assert_eq!(report.labels()[1].source().identity(), "src/first.rs");
    assert_eq!(report.help(), Some("rename one declaration"));
}

#[test]
fn report_retains_external_positions_without_claiming_source_text() {
    let source = Source::new("src/main.rs", "use support::Deferred;\n").unwrap();
    let external = ExternalPosition::checked("support/src/lib.rs", 41, 49, (3, 4), (3, 12)).unwrap();
    let report = Report::error(code::RESOLVE_UNKNOWN_IMPORT, "deferred Rust import")
        .primary(source, Span::new(Position::new(1, 5), Position::new(1, 22)), Some("Rawr use is here".into()))
        .unwrap()
        .secondary_external(external, Some("Rust declaration is here".into()))
        .finish();

    assert_eq!(report.labels().len(), 1);
    assert_eq!(report.sources().len(), 1);
    assert_eq!(report.external_labels().len(), 1);
    let retained = report.external_labels()[0].position();
    assert_eq!(retained.file(), "support/src/lib.rs");
    assert_eq!(retained.byte_range(), 41..49);
    assert_eq!(retained.positions(), ((3, 4), (3, 12)));
    assert_eq!(report.view().external_labels()[0].message(), Some("Rust declaration is here"));
}

#[test]
fn report_rejects_duplicate_primary_and_out_of_bounds_spans() {
    let source = Source::new("main.rs", "one\ntwo").unwrap();
    let builder = Report::error(code::PARSE_SYNTAX, "broken")
        .primary(source.clone(), Span::new(Position::new(1, 1), Position::new(1, 2)), None)
        .unwrap();
    assert_eq!(builder.primary(source.clone(), Span::new(Position::new(2, 1), Position::new(2, 2)), None,).unwrap_err(), Error::MultiplePrimaryLabels);
    assert!(matches!(Report::error(code::PARSE_SYNTAX, "broken").primary(source, Span::new(Position::new(3, 1), Position::new(3, 1)), None,), Err(Error::SpanOutsideSource { .. })));
}

#[test]
fn unpositioned_report_still_retains_its_exact_source_context() {
    let source = Source::new("main.rs", "pub fn helper() {}\n").unwrap();
    let report = Report::error(code::FRAMEWORK_MISSING_ENTRY, "missing application entry").source(source.clone()).finish();

    assert!(report.labels().is_empty());
    assert_eq!(report.sources(), &[source]);
    assert_eq!(report.view().sources()[0].identity(), "main.rs");
}

#[test]
fn duplicate_item_adapter_preserves_both_exact_spans() {
    let text = "struct Counter {}\nstruct Counter {}\n";
    let error = Engine::new().compile(text).unwrap_err();
    let report = Report::from_parse(Source::new("main.rs", text).unwrap(), &error).unwrap();

    assert_eq!(report.code(), code::RESOLVE_DUPLICATE_ITEM);
    assert_eq!(report.labels().len(), 2);
    assert_eq!(report.labels()[0].span(), error.primary_span());
    assert_eq!(report.labels()[1].span(), error.secondary_span().unwrap());
    assert!(matches!(error.err_type(), ParseErrorType::Resolution(rawr::resolve::Error::DuplicateItem { .. })));
}

#[test]
fn unknown_type_adapter_is_actionable_and_positioned() {
    let text = "struct Counter { value: Missing }\n";
    let error = Engine::new().compile(text).unwrap_err();
    let report = Report::from_parse(Source::new("main.rs", text).unwrap(), &error).unwrap();

    assert_eq!(report.code(), code::RESOLVE_UNKNOWN_TYPE);
    assert_eq!(report.labels().len(), 1);
    assert!(report.help().unwrap().contains("import it with `use`"));
}

#[test]
fn unknown_qualified_type_says_it_is_not_available_yet() {
    let text = "struct Counter { value: lib::widget::Missing }\n";
    let error = Engine::new().compile(text).unwrap_err();
    let report = Report::from_parse(Source::new("main.rs", text).unwrap(), &error).unwrap();

    assert_eq!(report.code(), code::RESOLVE_UNKNOWN_TYPE);
    assert_eq!(
        report.help().unwrap(),
        "`lib::widget::Missing` isn't available in Rawr yet, or the path is misspelled"
    );
}

#[cfg(feature = "serde")]
#[test]
fn serde_exports_only_the_stable_read_only_view() {
    let report = Report::error(code::ENVIRONMENT_PROJECT_NOT_FOUND, "not found").help("pass a source").finish();
    let json = serde_json::to_string(&report.view()).unwrap();

    assert!(json.contains("RAWR-ENVIRONMENT-PROJECT-NOT-FOUND"));
    assert!(json.contains("\"category\":\"environment\""));
    assert!(json.contains("\"labels\":[]"));
}

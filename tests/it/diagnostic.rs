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
fn unknown_type_adapter_reads_as_rustcs_e0425() {
    let text = "struct Counter { value: Missing }\n";
    let error = Engine::new().compile(text).unwrap_err();
    let report = Report::from_parse(Source::new("main.rs", text).unwrap(), &error).unwrap();

    assert_eq!(report.code().as_str(), "E0425");
    assert_eq!(report.message(), "cannot find type `Missing` in this scope");
    assert_eq!(report.labels().len(), 1);
    assert_eq!(report.labels()[0].message(), Some("not found in this scope"));
    assert_eq!(report.help(), None);
}

/// `self` where no method's receiver is reads as rustc's: a free function's
/// `self` parameter without a code, a `self` value as E0424.
#[test]
fn self_outside_a_method_reads_as_rustcs() {
    for (text, code, message, label, notes) in [
        (
            "fn view(&self) -> i64 { 0 }\nfn main() {}\n",
            "",
            "`self` parameter is only allowed in associated functions",
            "not semantically valid as function parameter",
            vec!["associated functions are those in `impl` or `trait` definitions"],
        ),
        (
            "fn view() -> i64 { self }\nfn main() {}\n",
            "E0424",
            "expected value, found module `self`",
            "`self` value is a keyword only available in methods with a `self` parameter",
            vec![],
        ),
    ] {
        let error = Engine::new().compile(text).unwrap_err();
        let report = Report::from_parse(Source::new("main.rs", text).unwrap(), &error).unwrap();
        assert_eq!(report.code().as_str(), code, "{text}");
        assert_eq!(report.message(), message, "{text}");
        assert_eq!(report.labels()[0].message(), Some(label), "{text}");
        assert_eq!(report.notes().collect::<Vec<_>>(), notes, "{text}");
    }
}

/// rustc's code, message, primary label (its text and its columns) and help
/// for each unresolved path: a crate the program doesn't depend on, an item
/// or a module a present module lacks.
#[test]
fn unresolved_paths_read_as_rustcs() {
    const CARGO_ADD: &str = "use `cargo add ratatui` to add it to your `Cargo.toml`";
    for (text, code, message, label, columns, help) in [
        (
            "use ratatui::buffer::Buffer;\n",
            "E0433",
            "cannot find module or crate `ratatui` in this scope",
            "use of unresolved module or unlinked crate `ratatui`",
            (5, 12),
            Some(format!("if you wanted to use a crate named `ratatui`, {CARGO_ADD}")),
        ),
        (
            "struct Counter { value: ratatui::buffer::Buffer }\n",
            "E0433",
            "cannot find module or crate `ratatui` in this scope",
            "use of unresolved module or unlinked crate `ratatui`",
            (25, 32),
            Some(format!("if you wanted to use a crate named `ratatui`, {CARGO_ADD}")),
        ),
        (
            "use ratatui::*;\n",
            "E0432",
            "unresolved import `ratatui`",
            "use of unresolved module or unlinked crate `ratatui`",
            (5, 12),
            Some(format!("if you wanted to use a crate named `ratatui`, {CARGO_ADD}")),
        ),
        ("use ratatui;\n", "E0432", "unresolved import `ratatui`", "no external crate `ratatui`", (5, 12), None),
        ("mod lib { pub mod widget {} }\nuse lib::widget::Missing;\n", "E0432", "unresolved import `lib::widget::Missing`", "no `Missing` in `lib::widget`", (18, 25), None),
        ("mod lib { pub mod widget {} }\nuse lib::nothing::Missing;\n", "E0432", "unresolved import `lib::nothing`", "could not find `nothing` in `lib`", (10, 17), None),
        (
            "mod lib { pub mod widget {} }\nstruct Counter { value: lib::widget::Missing }\n",
            "E0425",
            "cannot find type `Missing` in module `lib::widget`",
            "not found in `lib::widget`",
            (38, 45),
            None,
        ),
    ] {
        let error = Engine::new().compile(text).unwrap_err();
        let report = Report::from_parse(Source::new("main.rs", text).unwrap(), &error).unwrap();
        let primary = &report.labels()[0];
        let at = (primary.span().start().position(), primary.span().end().position());

        assert_eq!(report.code().as_str(), code, "{text}");
        assert_eq!(report.message(), message, "{text}");
        assert_eq!(primary.message(), Some(label), "{text}");
        assert_eq!(at, (Some(columns.0), Some(columns.1)), "{text}");
        assert_eq!(report.help(), help.as_deref(), "{text}");
    }
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

/// rustc's codes for std paths (D711): E0432, E0433, E0425, E0658, and
/// rawr's own for an item it can't run yet.
#[test]
fn std_path_errors_carry_rustcs_codes() {
    for (text, code, message) in [
        ("use core::collections::HashMap;\n", "E0432", "unresolved import `core::collections`"),
        ("fn f() { core::thread::sleep(); }\n", "E0433", "cannot find `thread` in `core`"),
        ("struct S { at: core::time::Instant }\n", "E0425", "cannot find type `Instant` in module `core::time`"),
        ("use std::intrinsics::abort;\n", "E0658", "use of unstable library feature `core_intrinsics`"),
        ("struct S { lock: std::sync::RwLock<i64> }\n", "RAWR-NOT-YET", "rawr doesn't run `std::sync::RwLock` yet"),
    ] {
        let error = Engine::new().compile(text).unwrap_err();
        let report = Report::from_parse(Source::new("main.rs", text).unwrap(), &error).unwrap();
        assert_eq!(report.code().as_str(), code, "{text}");
        assert!(report.message().starts_with(message), "{text}: {}", report.message());
    }
    let text = "struct S { values: Vec<i64, std::alloc::Global> }\n";
    let error = Engine::new().compile(text).unwrap_err();
    let report = Report::from_parse(Source::new("main.rs", text).unwrap(), &error).unwrap();
    assert_eq!(report.code().as_str(), "E0658");
    assert_eq!(report.notes().collect::<Vec<_>>(), ["see issue #32838 <https://github.com/rust-lang/rust/issues/32838> for more information"]);
}

use std::path::Path;

use rawr::diagnostic::{code, Category, LabelKind};
use rawr::project;

#[test]
fn runtime_view_failure_retains_canonical_source_report_and_overlay() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/diagnostics/view_error.rawr");
    let target = project::Target::discover(&path).unwrap();
    let prepared = rawr_iced::application::Compiler::new()
        .unwrap()
        .check(&target)
        .unwrap()
        .prepare()
        .unwrap();
    let entry = &prepared.adapter().diagnostics()[0];
    let report = entry.report();

    assert_eq!(report.code(), code::RUNTIME_THROW);
    assert_eq!(report.category(), Category::Runtime);
    assert_eq!(report.message(), "runtime error: initial view failed");
    assert_eq!(report.labels().len(), 1);
    assert_eq!(report.labels()[0].kind(), LabelKind::Primary);
    assert_eq!(report.labels()[0].span().start().line(), Some(19));
    assert_eq!(
        report.labels()[0].source().identity(),
        path.to_string_lossy()
    );
    assert_eq!(report.labels()[0].source().text(), target.source().text());

    let mut simulator = iced_test::simulator(prepared.adapter().view());
    assert!(simulator.find("Rawr runtime diagnostic").is_ok());
    assert!(simulator
        .find("view · runtime · error[RAWR-RUNTIME-THROW]")
        .is_ok());
    assert!(simulator.find("runtime error: initial view failed").is_ok());
}

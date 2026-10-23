//! Public-boundary coverage for the interpreter-independent host controls.

use rawr_iced::control;
use rawr_iced::preview::Compiler;

#[test]
fn prepared_host_exposes_only_stable_control_data_and_operations() {
    let mut host = Compiler::new()
        .unwrap()
        .check_named_source("public-control.rs", source())
        .unwrap()
        .prepare()
        .unwrap()
        .into_control();
    let preview = &host.previews()[0];
    assert_eq!(preview.name(), "Public");
    assert_eq!(preview.group(), "boundary");
    assert_eq!((preview.width(), preview.height()), (90, 70));
    let key = preview.key().clone();

    assert!(matches!(
        host.status().selection(),
        control::Selection::Preview(selected) if selected == &key
    ));
    assert!(host.status().is_active());
    assert!(host.diagnostics().is_empty());

    assert!(!host.close().is_active());
    assert!(host.reset().unwrap().is_active());
    assert!(host.select(&key).unwrap().is_active());
    assert!(matches!(
        host.snapshot(rawr_iced::snapshot::Mode::Check),
        Err(control::Error::Root)
    ));
}

fn source() -> &'static str {
    r#"
use iced::Element;
use iced::widget::text;

#[preview(name = "Public", group = "boundary", size = (90, 70))]
fn sample() -> Element<'_, ()> { text("public control") }
"#
}

#[path = "../fixtures/dial/src/lib.rs"]
mod dial;

use dial::Dial;
use iced::widget::Widget as _;

#[test]
fn unchanged_dial_converts_draws_layouts_and_publishes_its_owned_message() {
    let dial = Dial::new(4, |value| value).step(3);
    let element: iced::Element<'_, u8> = dial.boxed();
    let mut simulator = iced_test::simulator(element);
    simulator.point_at(iced::Point::new(18.0, 18.0));
    let _ = simulator.simulate(iced_test::simulator::click());
    assert_eq!(simulator.into_messages().collect::<Vec<_>>(), [7]);
}

#[test]
fn dial_manifest_and_source_have_no_rawr_registration_burden() {
    let manifest = include_str!("../fixtures/dial/Cargo.toml");
    let source = include_str!("../fixtures/dial/src/lib.rs");
    for forbidden in [
        "rawr =",
        "rawr_import",
        "build =",
        "#[rawr",
        "#[derive(Rawr",
    ] {
        assert!(
            !manifest.contains(forbidden),
            "manifest contains {forbidden}"
        );
        assert!(!source.contains(forbidden), "source contains {forbidden}");
    }
}

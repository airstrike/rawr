#[test]
fn ordinary_iced_style_function_items_remain_native_function_items() {
    use iced::widget::Widget as _;

    let primary: iced::Element<'_, ()> = iced::widget::button("Save")
        .style(iced::widget::button::primary)
        .boxed();
    let danger: iced::Element<'_, ()> = iced::widget::button("Delete")
        .style(iced::widget::button::danger)
        .boxed();
    let _ = iced::widget::row![primary, danger]
        .spacing(8)
        .align_y(iced::Alignment::Center);

    let limits = rawr_import::cargo::Limits::standard();
    let definition = rawr_iced::profile::Definition::new(&limits).unwrap();
    let graph = rawr_import::test_fixture::iced_profile();
    let contract = rawr_import::profile::classify(&definition, &graph, &limits).unwrap();
    let checked = rawr_iced::profile::Checked::from_fingerprint([0x73; 32]);
    let demand = rawr_import::profile::demand(&definition, &contract, &checked, &limits).unwrap();
    let proof = rawr_import::profile::Proof::generate(&definition, &demand, &limits).unwrap();
    let source = std::str::from_utf8(proof.generated_source()).unwrap();
    assert!(!source.contains("button::primary"));
    assert!(!source.contains("button::danger"));
}

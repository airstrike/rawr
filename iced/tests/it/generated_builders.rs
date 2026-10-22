#[test]
fn ordinary_iced_consuming_builders_compile_without_profile_inventory() {
    use iced::widget::Widget as _;

    let save = iced::widget::button("Save")
        .padding([8, 12])
        .on_press(())
        .width(iced::Length::Shrink);
    let input = iced::widget::text_input("Name", "Rawr")
        .padding(8)
        .on_input(|_| ());
    let checkbox = iced::widget::checkbox(true)
        .label("Ready")
        .on_toggle(|_| ());
    let toggler = iced::widget::toggler(false).label("Live").on_toggle(|_| ());
    let slider = iced::widget::slider(0..=10, 4, |_| ()).step(2);
    let scrollable = iced::widget::scrollable(iced::widget::column![
        save, input, checkbox, toggler, slider
    ]);
    let _: iced::Element<'_, ()> = iced::widget::container(scrollable).boxed();

    let limits = rawr_import::cargo::Limits::standard();
    let definition = rawr_iced::profile::Definition::new(&limits).unwrap();
    let graph = rawr_import::test_fixture::iced_profile();
    let contract = rawr_import::profile::classify(&definition, &graph, &limits).unwrap();
    assert_eq!(contract.evidence().len(), 19);
}

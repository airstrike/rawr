//! A selection of widgets, as the `rawr-iced` command makes for a project
//! whose iced has only some: the facade it declares binds. Its own test
//! binary, since a selection holds for the whole process.

use rawr_iced::features::{self, Feature};

#[test]
fn a_selected_subset_of_widgets_declares_and_binds() {
    features::select([Feature::Canvas, Feature::Svg]);
    assert!(features::on(Feature::Canvas));
    assert!(!features::on(Feature::Image));
    rawr_iced::facade().expect("the facade builds");
    rawr_iced::runtime_facades().expect("every declared item binds");
}

//! Places, as iced_core's `widget::tree` diffs a widget's children: generic
//! code that `events`' generic code calls through `events`' re-export of
//! this module, as iced_widget's `crate::core::widget::tree`.

/// Each of `items` after its place, as `tree::diff_children` walks any
/// widget's slice.
pub fn tallied<T: std::fmt::Display>(items: &[T]) -> String {
    items
        .iter()
        .enumerate()
        .map(|(at, item)| format!("{at}:{item}"))
        .collect::<Vec<_>>()
        .join(" ")
}

//! Native: the app's widgets, as inboard's `widget`, with a `text` of its
//! own that a marked module imports beside `theme`.
pub fn text(content: &str) -> String {
    format!("text {content}")
}

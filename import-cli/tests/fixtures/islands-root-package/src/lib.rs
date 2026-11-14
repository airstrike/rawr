//! The workspace's root package.

/// What the shelf says.
pub fn label() -> String {
    format!("shelf {}", shelf_core::success())
}

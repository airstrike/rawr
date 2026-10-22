//! Portable cache-directory names derived only from a build request.

/// Encode one opaque build key as its fixed lowercase directory name.
pub(crate) fn directory(key: crate::bridge::BuildRequestKey) -> String {
    let mut name = String::with_capacity(64);
    use std::fmt::Write as _;
    for byte in key.as_bytes() {
        let _ = write!(name, "{byte:02x}");
    }
    name
}

//! An optional dependency of `islands-assets`, which its `loud` feature
//! enables with this crate's own `loud`.
pub fn shout(text: &str) -> String {
    if cfg!(feature = "loud") {
        text.to_uppercase()
    } else {
        text.to_owned()
    }
}

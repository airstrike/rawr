//! Native code that the package's features decide: `greet` and `loud` are
//! its default features, and `loud` enables the optional `extra` with its
//! own `loud` (#1144).
#[cfg(feature = "greet")]
pub fn greeting() -> &'static str {
    "hello"
}

#[cfg(feature = "loud")]
pub fn loud() -> String {
    extra::shout("loud")
}

#[cfg(not(feature = "loud"))]
pub fn loud() -> String {
    "quiet".to_owned()
}

pub fn never() -> bool {
    cfg!(feature = "never")
}

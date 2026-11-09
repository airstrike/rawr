//! Native: the screens. `home` is marked, `about` calls into it, and
//! `other` is private to `screen`, as `OFFSET` is: rustc lets `home` name
//! both through `super::`.
pub mod about;
#[allow(dead_code)] // `home` is the app's screen under work.
pub mod home;
mod other;

const OFFSET: usize = 0;

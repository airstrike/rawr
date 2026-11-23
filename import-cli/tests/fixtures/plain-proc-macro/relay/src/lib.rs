//! A library that re-exports another library's macro, whose `$crate` is
//! a crate the program that names this one need not depend on.

pub use wrap::relayed;

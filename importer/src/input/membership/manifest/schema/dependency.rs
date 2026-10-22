//! Complete pinned dependency interpretation without workspace resolution.
//!
//! Cargo schema mod.rs:695–712 captures serde-value before its inherited trial;
//! mod.rs:718–731 and825–869 define every inherited/detailed field and flatten.
//! All payloads borrow the admitted Document; intermediate ownership is flat
//! and uses the caller's original Session through both semantic attempts.

pub(in super::super) mod direct;
pub(in super::super) mod inheritable;
mod map;
mod record;

pub(in super::super) use direct::Direct;
pub(in super::super) use inheritable::Inheritable;
pub(in super::super) use record::{Detailed, Entry, Field, Inherited};

#[cfg(test)]
pub(in super::super) mod reference;
#[cfg(test)]
mod tests;

//! Unicode identifier classes, as Cargo checks package, target and feature
//! names.
//!
//! The exact classes come from `unicode-ident` under `unicode-names`, which
//! every command that reads manifests has. A project's runner reads none, and
//! links this crate without it: the runner's `unicode-ident` would be a
//! target unit, and Cargo builds the project's own `unicode-ident`, a host
//! unit of every proc macro, with another profile, so the project's syn and
//! every proc macro above it would build twice. Without the feature a
//! non-ASCII character is classed by `char::is_alphabetic` and
//! `char::is_alphanumeric`.

/// Whether `character` may start an identifier.
#[cfg(feature = "unicode-names")]
pub(crate) fn is_xid_start(character: char) -> bool {
    unicode_ident::is_xid_start(character)
}

/// Whether `character` may continue an identifier.
#[cfg(feature = "unicode-names")]
pub(crate) fn is_xid_continue(character: char) -> bool {
    unicode_ident::is_xid_continue(character)
}

/// Whether `character` may start an identifier.
#[cfg(not(feature = "unicode-names"))]
pub(crate) fn is_xid_start(character: char) -> bool {
    character.is_ascii_alphabetic() || (!character.is_ascii() && character.is_alphabetic())
}

/// Whether `character` may continue an identifier.
#[cfg(not(feature = "unicode-names"))]
pub(crate) fn is_xid_continue(character: char) -> bool {
    character.is_ascii_alphanumeric()
        || character == '_'
        || (!character.is_ascii() && character.is_alphanumeric())
}

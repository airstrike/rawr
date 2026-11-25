//! Helpers that make frames, as iced's `widget::helpers` makes widgets: a
//! private module whose items the crate's root re-exports by a glob, so
//! `events::framed` is both the module `framed` and the function here.

use crate::framed::{self, Framed, Shown};

/// `item` framed by hashes, through the `framed` module's own generic code.
pub fn framed<S: Shown>(item: S) -> Framed<S> {
    framed::framed(item).edge('#')
}

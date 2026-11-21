//! The shared cells native code holds script values in, which a save that
//! changes a script type's fields reaches to rewrite the values in place
//! (#2026). A runner's proxy of a script type, the state a native generic
//! `run` keeps as `S`, holds its value in a cell native code lends to each
//! call; the cell is noted when it is made (`reload::live`), and a save
//! rewrites the value inside it, so every handle to it stays valid.

use crate::types::dynamic::Union;
use crate::Dynamic;

/// Note the cell `value` is, when it is one, among the live cells a
/// reload reaches (`reload::live`).
pub(crate) fn hold(value: &Dynamic) {
    let Union::Shared(cell, ..) = &value.0 else {
        return;
    };
    crate::reload::live::live()
        .cells
        .push(crate::Shared::downgrade(cell));
}

/// Each noted cell still alive, as a handle to it.
pub(crate) fn alive() -> Vec<Dynamic> {
    let cells = crate::reload::live::live().cells.alive();
    cells.into_iter().map(Dynamic::from).collect()
}

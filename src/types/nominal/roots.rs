//! The shared cells native code holds script values in, which a save that
//! changes a script type's fields reaches to rewrite the values in place
//! (#2026). A runner's proxy of a script type, the state a native generic
//! `run` keeps as `S`, holds its value in a cell native code lends to each
//! call; the cell is noted here when it is made, and a save rewrites the
//! value inside it, so every handle to it stays valid.

use crate::types::dynamic::Union;
use crate::Dynamic;

type Weak = std::sync::Weak<crate::Locked<Dynamic>>;

/// The cells noted, and how many were alive when they were last swept.
#[derive(Default)]
struct Cells {
    cells: Vec<Weak>,
    swept: usize,
}

static CELLS: std::sync::Mutex<Cells> = std::sync::Mutex::new(Cells {
    cells: Vec::new(),
    swept: 0,
});

fn with_cells<R>(work: impl FnOnce(&mut Cells) -> R) -> R {
    work(
        &mut CELLS
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner),
    )
}

/// Note the cell `value` is, when it is one. The cells that went are swept
/// once as many were noted as lived at the last sweep, so the list stays
/// within twice the live ones.
pub(crate) fn hold(value: &Dynamic) {
    let Union::Shared(cell, ..) = &value.0 else {
        return;
    };
    let weak = crate::Shared::downgrade(cell);
    with_cells(|cells| {
        if cells.cells.len() >= cells.swept.max(16) * 2 {
            cells.cells.retain(|cell| cell.strong_count() > 0);
            cells.swept = cells.cells.len();
        }
        cells.cells.push(weak);
    });
}

/// Each noted cell still alive, as a handle to it.
pub(crate) fn alive() -> Vec<Dynamic> {
    with_cells(|cells| {
        cells.cells.retain(|cell| cell.strong_count() > 0);
        cells.swept = cells.cells.len();
        cells
            .cells
            .iter()
            .filter_map(Weak::upgrade)
            .map(Dynamic::from)
            .collect()
    })
}

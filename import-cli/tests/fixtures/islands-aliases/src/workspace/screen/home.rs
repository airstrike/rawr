// rawr
//! Marked: names another crate's type only through a workspace crate's
//! alias, by a module a `use` binds, in a field, a parameter and a `let`,
//! and std types through a crate's alias and a native module's.
use model::extension;

pub struct Board {
    pub updates: extension::registry::Updates,
}

impl Board {
    pub fn newest<'a>(&'a self, updates: &'a [extension::registry::Update]) -> u64 {
        updates
            .iter()
            .map(|update| update.version)
            .max()
            .unwrap_or(0)
    }
}

pub fn summary(first: extension::registry::Update, second: extension::registry::Update) -> String {
    let board = Board {
        updates: vec![first, second],
    };
    let first: &extension::registry::Update = &board.updates[0];
    let names: extension::registry::Names = extension::registry::names();
    let labels: crate::workspace::Labels = crate::workspace::labels();
    format!(
        "{} {} / {} of {} / {} / {}",
        first.name,
        first.version,
        board.newest(&board.updates),
        board.updates.len(),
        names.join(","),
        labels.join(",")
    )
}

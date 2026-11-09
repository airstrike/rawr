//! Native: the project registry, as inboard's `model::project::Registry`:
//! entries the marked home screen lists by when they were last opened.

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Stamp(pub u64);

impl Stamp {
    pub fn humanize(&self) -> String {
        format!("{}m ago", 100 - self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Id(pub u32);

#[derive(Debug)]
pub struct Entry {
    id: Id,
    name: String,
    opened_at: Stamp,
    trashed: bool,
    shared: bool,
}

impl Entry {
    pub fn id(&self) -> Id {
        self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn opened_at(&self) -> Stamp {
        self.opened_at
    }

    pub fn is_trashed(&self) -> bool {
        self.trashed
    }

    pub fn is_shared(&self) -> bool {
        self.shared
    }
}

#[derive(Debug, Default)]
pub struct Registry {
    entries: Vec<Entry>,
}

impl Registry {
    pub fn add(&mut self, name: &str, opened_at: u64, trashed: bool, shared: bool) {
        let id = Id(u32::try_from(self.entries.len()).unwrap_or(u32::MAX));
        self.entries.push(Entry {
            id,
            name: name.to_owned(),
            opened_at: Stamp(opened_at),
            trashed,
            shared,
        });
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }
}

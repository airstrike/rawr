//! Cargo's successful unused-key result, preserving the callback encounter log.
//!
//! `util/toml/mod.rs:196–232` renders map keys literally and sequence positions
//! in decimal, omits Some/newtype wrappers, then inserts rendered paths into a
//! BTreeSet. Strings are owned once here; the sorted set indexes that ownership.

use std::alloc::Layout;

use rawr_toml::document;

use crate::input::membership::manifest::schema::{package, profile, workspace};
use crate::input::membership::manifest::{session, storage, value, Error, Step};

use super::{Event, Original, Value};

#[cfg(test)]
mod tests;

pub(in crate::input::membership::manifest) struct Unused {
    ordered: storage::Buffer<String>,
    unique: storage::Buffer<usize>,
}

impl Unused {
    pub(super) const fn new() -> Self {
        Self {
            ordered: storage::Buffer::new(),
            unique: storage::Buffer::new(),
        }
    }

    pub(in crate::input::membership::manifest) fn ordered(
        &self,
    ) -> impl ExactSizeIterator<Item = &str> {
        self.ordered.as_slice().iter().map(String::as_str)
    }

    pub(in crate::input::membership::manifest) fn unique(
        &self,
    ) -> impl ExactSizeIterator<Item = &str> {
        self.unique
            .as_slice()
            .iter()
            .map(|&index| self.ordered.as_slice()[index].as_str())
    }
}

struct Collector<'d> {
    output: Unused,
    path: storage::Buffer<package::Position<'d>>,
    ancestors: storage::Buffer<usize>,
}

pub(super) fn collect<A: document::Admission>(
    original: &Original<'_>,
    session: &mut session::Session<'_, A>,
) -> Step<Unused> {
    session.check()?;
    let mut collector = Collector {
        output: Unused::new(),
        path: storage::Buffer::new(),
        ancestors: storage::Buffer::new(),
    };
    for event in original.events.as_slice() {
        match *event {
            Event::Ignored(key) => collector.key(key, session)?,
            Event::Field(index) => {
                let Some(entry) = original.entries.as_slice().get(index) else {
                    return session.fail(Error::State);
                };
                collector.path.push(entry.position(), session)?;
                collector.field(entry.value(), session)?;
                collector.path.clear();
            }
        }
    }
    for index in 0..collector.output.ordered.len() {
        collector.output.unique.push(index, session)?;
    }
    let Unused { ordered, unique } = &mut collector.output;
    storage::sort(unique.as_mut_slice(), |&left, &right| {
        ordered.as_slice()[left].cmp(&ordered.as_slice()[right])
    });
    let mut retained = 0;
    for index in 0..unique.len() {
        if retained == 0
            || ordered.as_slice()[unique.as_slice()[retained - 1]]
                != ordered.as_slice()[unique.as_slice()[index]]
        {
            unique.as_mut_slice().swap(retained, index);
            retained += 1;
        }
    }
    unique.truncate(retained);
    Ok(collector.output)
}

impl<'d> Collector<'d> {
    fn field<A: document::Admission>(
        &mut self,
        value: &Value<'d>,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        match value {
            Value::Package(package) => {
                for ignored in package.ignored() {
                    self.package(*ignored, session)?;
                }
            }
            Value::Target(target) => {
                for &key in target.unused() {
                    self.key(key, session)?;
                }
            }
            Value::Targets(targets) => {
                for ignored in targets.ignored() {
                    self.path
                        .push(package::Position::Sequence(ignored.index()), session)?;
                    self.key(*ignored.field(), session)?;
                    self.path.pop();
                }
            }
            Value::Platforms(platforms) => {
                for ignored in platforms.ignored() {
                    self.path
                        .push(package::Position::Map(*ignored.parent()), session)?;
                    self.key(*ignored.field(), session)?;
                    self.path.pop();
                }
            }
            Value::Hints(hints) => {
                for &key in hints.unused() {
                    self.key(key, session)?;
                }
            }
            Value::Workspace(workspace) => {
                for ignored in workspace.ignored() {
                    match *ignored {
                        workspace::Ignored::Field(key) => self.key(key, session)?,
                        workspace::Ignored::Package { position, path } => {
                            self.path.push(position, session)?;
                            self.package(path, session)?;
                            self.path.pop();
                        }
                    }
                }
            }
            Value::Profiles(profiles) => self.profiles(profiles, session)?,
            // These visitors consume their values. Flattened dependency/lint
            // extras are separately retained data, not serde_ignored callbacks.
            Value::Primitive(_)
            | Value::Features(_)
            | Value::Dependencies(_)
            | Value::Lints(_)
            | Value::Patch(_)
            | Value::Replace(_) => {}
        }
        Ok(())
    }

    fn package<A: document::Admission>(
        &mut self,
        ignored: package::Ignored<'d>,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        let length = self.path.len();
        if let Some(parent) = ignored.parent() {
            self.path.push(parent, session)?;
        }
        self.key(*ignored.key(), session)?;
        self.path.truncate(length);
        Ok(())
    }

    fn profiles<A: document::Admission>(
        &mut self,
        profiles: &profile::Profiles<'d>,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        let prefix = self.path.len();
        for (mut node, key) in profiles.ignored() {
            self.ancestors.clear();
            loop {
                let Some(profile) = profiles.nodes().get(node) else {
                    return session.fail(Error::State);
                };
                self.ancestors.push(node, session)?;
                match profile.location() {
                    profile::Location::Root(_) => break,
                    profile::Location::Package { parent, .. }
                    | profile::Location::BuildOverride { parent, .. } => {
                        // A parent is created before its child. The check also
                        // makes malformed ancestry fail without cyclic work.
                        if parent >= node {
                            return session.fail(Error::State);
                        }
                        node = parent;
                    }
                }
            }
            while let Some(node) = self.ancestors.pop() {
                match profiles.node(node).location() {
                    profile::Location::Root(key) => {
                        self.path.push(package::Position::Map(key), session)?;
                    }
                    profile::Location::Package { position, key, .. } => {
                        self.path.push(position, session)?;
                        self.path.push(package::Position::Map(key), session)?;
                    }
                    profile::Location::BuildOverride { position, .. } => {
                        self.path.push(position, session)?;
                    }
                }
            }
            self.key(*key, session)?;
            self.path.truncate(prefix);
        }
        Ok(())
    }

    fn key<A: document::Admission>(
        &mut self,
        key: value::Key<'d>,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        self.path.push(package::Position::Map(key), session)?;
        let text = render(self.path.as_slice(), session)?;
        self.output.ordered.push(text, session)?;
        self.path.pop();
        Ok(())
    }
}

fn render<A: document::Admission>(
    path: &[package::Position<'_>],
    session: &mut session::Session<'_, A>,
) -> Step<String> {
    session.check()?;
    let mut length = 0usize;
    for component in path {
        let bytes = match component {
            package::Position::Map(key) => key.text.as_str().len(),
            package::Position::Sequence(index) => digits(*index),
        };
        let Some(next) = length
            .checked_add(usize::from(length != 0))
            .and_then(|length| length.checked_add(bytes))
        else {
            return session.fail(Error::Overflow);
        };
        length = next;
    }
    session.string(length)?;
    let Ok(layout) = Layout::array::<u8>(length) else {
        return session.fail(Error::Overflow);
    };
    if length != 0 {
        session.admit(document::Request::Allocation {
            bytes: layout.size(),
            alignment: layout.align(),
        })?;
    }
    let mut output = String::new();
    if let Err(error) = output.try_reserve_exact(length) {
        return session.fail(Error::Allocation(error));
    }
    #[cfg(test)]
    if length != 0 {
        ALLOCATIONS.with(|count| count.set(count.get() + 1));
    }
    for component in path {
        if !output.is_empty() {
            output.push('.');
        }
        match component {
            package::Position::Map(key) => output.push_str(key.text.as_str()),
            package::Position::Sequence(index) => {
                // A usize needs no more decimal digits than it has bits.
                let mut bytes = [0; usize::BITS as usize];
                let mut start = bytes.len();
                let mut remaining = *index;
                loop {
                    start -= 1;
                    bytes[start] = b'0' + (remaining % 10) as u8;
                    remaining /= 10;
                    if remaining == 0 {
                        break;
                    }
                }
                let Ok(text) = std::str::from_utf8(&bytes[start..]) else {
                    return session.fail(Error::State);
                };
                output.push_str(text);
            }
        }
    }
    if output.len() != length {
        return session.fail(Error::State);
    }
    Ok(output)
}

fn digits(mut value: usize) -> usize {
    let mut digits = 1;
    while value >= 10 {
        value /= 10;
        digits += 1;
    }
    digits
}

#[cfg(test)]
thread_local! {
    static ALLOCATIONS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

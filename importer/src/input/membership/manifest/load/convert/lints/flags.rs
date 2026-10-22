// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../../path/LICENSE-MIT and ../../path/LICENSE-APACHE.
//! Ordered lint options; check-cfg conversion must follow before completion.
use crate::input::membership::manifest::{
    load::normalize::target, schema::lints, session::Session, storage, Step,
};
use rawr_toml::document;

pub(in super::super::super) struct Ordered<'a> {
    entries: storage::Buffer<Entry<'a>>,
}
struct Entry<'a> {
    priority: i8,
    name: &'a str,
    option: String,
}
/// Owned base flags; the caller must still perform reached check-cfg conversion.
pub(in super::super::super) struct Flags(storage::Buffer<String>);
impl Flags {
    pub(in super::super::super) fn as_slice(&self) -> &[String] {
        self.0.as_slice()
    }

    pub(in super::super::super) fn append<A: document::Admission>(
        mut self,
        check_cfg: storage::Buffer<String>,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        for value in check_cfg.into_iter() {
            session.items(1)?;
            let flag = target::formatted(format_args!("--check-cfg"), session)?;
            session.items(self.0.len())?;
            self.0.push(flag, session)?;
            session.items(self.0.len())?;
            self.0.push(value, session)?;
        }
        Ok(self)
    }
}
impl Ordered<'_> {
    pub(in super::super::super) fn into_flags<A: document::Admission>(
        self,
        session: &mut Session<'_, A>,
    ) -> Step<Flags> {
        session.check()?;
        let mut flags = storage::Buffer::new();
        for entry in self.entries.into_iter() {
            session.items(1)?;
            session.items(flags.len())?;
            flags.push(entry.option, session)?;
        }
        Ok(Flags(flags))
    }
    pub(in super::super::super) fn options(&self) -> impl Iterator<Item = &str> {
        self.entries
            .as_slice()
            .iter()
            .map(|entry| entry.option.as_str())
    }
}

pub(in super::super::super) fn order<'a, A: document::Admission>(
    table: Option<&'a lints::Table<'_>>,
    session: &mut Session<'_, A>,
) -> Step<Ordered<'a>> {
    session.check()?;
    let mut entries = storage::Buffer::new();
    if let Some(table) = table {
        for (tool, rules) in table.tools() {
            session.items(1)?;
            let tool = tool.text.as_str();
            session.string(tool.len())?;
            if tool == "cargo" {
                continue;
            }
            for (name, config) in rules.rules() {
                session.items(1)?;
                let name = name.text.as_str();
                session.string(name.len())?;
                let (level, priority) = match config {
                    lints::Lint::Level(level) => (*level, 0),
                    lints::Lint::Config(config) => (config.level(), config.priority()),
                };
                let flag = match level {
                    lints::Level::Forbid => "--forbid",
                    lints::Level::Deny => "--deny",
                    lints::Level::Warn => "--warn",
                    lints::Level::Allow => "--allow",
                };
                let option = if tool == "rust" {
                    target::formatted(format_args!("{flag}={name}"), session)?
                } else {
                    target::formatted(format_args!("{flag}={tool}::{name}"), session)?
                };
                session.items(entries.len())?;
                entries.push(
                    Entry {
                        priority,
                        name,
                        option,
                    },
                    session,
                )?;
            }
        }
    }
    storage::try_sort(entries.as_mut_slice(), |left, right| {
        session.items(1)?;
        let priority = left.priority.cmp(&right.priority);
        if !priority.is_eq() {
            return Ok(priority);
        }
        session.items(left.name.len().min(right.name.len()))?;
        let name = right.name.cmp(left.name);
        if !name.is_eq() {
            return Ok(name);
        }
        session.items(left.option.len().min(right.option.len()))?;
        Ok(left.option.cmp(&right.option))
    })?;
    Ok(Ordered { entries })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference, value};
    #[test]
    fn flags_follow_priority_reverse_name_and_option_ties() {
        let text = "[lints.cargo]\nignored='deny'\n[lints.rust]\nall={level='warn',priority=-1}\nunused='deny'\nz='allow'\n[lints.clippy]\nunused='warn'\n[lints.other]\nunused='forbid'";
        let mut account = reference::Account;
        let document = document::parse(text, 100, &mut account).unwrap();
        let mut session = Session::new(&mut account);
        let table = lints::Table::interpret(
            value::Input::raw(document.table().get("lints").unwrap()),
            &mut session,
        )
        .unwrap();
        let ordered = order(Some(&table), &mut session).unwrap();
        assert_eq!(
            ordered.options().collect::<Vec<_>>(),
            [
                "--warn=all",
                "--allow=z",
                "--deny=unused",
                "--forbid=other::unused",
                "--warn=clippy::unused"
            ]
        );
    }

    #[test]
    fn flags_transfer_strings_and_append_check_cfg_pairs_without_reordering() {
        let mut account = reference::Account;
        let mut session = Session::new(&mut account);
        let mut entries = storage::Buffer::new();
        let option = String::from("--deny=unused");
        let option_pointer = option.as_ptr();
        entries
            .push(
                Entry {
                    priority: 0,
                    name: "unused",
                    option,
                },
                &mut session,
            )
            .unwrap();
        let flags = Ordered { entries }.into_flags(&mut session).unwrap();
        assert_eq!(flags.as_slice()[0].as_ptr(), option_pointer);
        let mut values = storage::Buffer::new();
        let first = String::from("cfg(x)");
        let pointer = first.as_ptr();
        values.push(first, &mut session).unwrap();
        values.push(String::from("cfg(x)"), &mut session).unwrap();
        let flags = flags.append(values, &mut session).unwrap();
        assert_eq!(
            flags.as_slice(),
            [
                "--deny=unused",
                "--check-cfg",
                "cfg(x)",
                "--check-cfg",
                "cfg(x)"
            ]
        );
        assert_eq!(flags.as_slice()[2].as_ptr(), pointer);
    }
}

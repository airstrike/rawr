//! What of the runner names the program's own crate (#1775): rawr's copy of
//! it, which a save to the program's sources recompiles. Cargo recompiles
//! every crate that depends on a recompiled one, so the runner's code that
//! names the program's crate is a member of its own, the program's
//! (`member::Member::Program`), and the base and the crates of adapters
//! that don't name it don't depend on it. A save to a held type then
//! recompiles rawr's copy, the program's member and the runner alone.
//!
//! An item names the program when its text names one of the program's
//! crates, a proxy of a script type (D594), which the program's member
//! declares, or another item of the program's.
//!
//! The program's crate is the script's, which the generator learns from the
//! script types it converts, so a runner over a program whose script types
//! never reach native code keeps one base that depends on it.

use std::collections::BTreeSet;

use toml_edit::{DocumentMut, Item};

use super::HiddenCrate;

/// The program's crates, as the runner names them.
pub(super) struct Program {
    /// The packages: rawr's copy of the program's crate, and each path
    /// package that depends on it, as its binary's.
    packages: BTreeSet<String>,
    /// The names the runner's sources reach them by: their dependency keys
    /// and the `__rawr_crate_..` segments of hidden ones.
    crates: BTreeSet<String>,
    /// The script types' proxies, as `crate::<path>`.
    proxies: Vec<String>,
    /// The items found to name the program, by name.
    named: BTreeSet<String>,
}

impl Program {
    /// The program's crates in the runner's `manifest` and `lock`, when the
    /// runner depends on the script's crate, `krate`.
    pub(super) fn find(
        manifest: &[u8],
        lock: &[u8],
        hidden: &[HiddenCrate],
        krate: &str,
        proxies: impl Iterator<Item = String>,
    ) -> Option<Self> {
        let manifest = std::str::from_utf8(manifest)
            .ok()?
            .parse::<DocumentMut>()
            .ok()?;
        let dependencies = dependencies(&manifest);
        let (_, script) = dependencies.iter().find(|(key, _)| key == krate)?;
        let mut packages = BTreeSet::from([script.clone()]);
        // Each path package that depends on one of them is the program's too.
        let lock = std::str::from_utf8(lock)
            .ok()?
            .parse::<DocumentMut>()
            .ok()?;
        let locked = lock
            .get("package")
            .and_then(Item::as_array_of_tables)
            .map(|packages| {
                packages
                    .iter()
                    .filter(|package| !package.contains_key("source"))
                    .filter_map(|package| {
                        let name = package.get("name")?.as_str()?.to_owned();
                        let needs = package
                            .get("dependencies")
                            .and_then(Item::as_array)
                            .map(|needs| {
                                needs
                                    .iter()
                                    .filter_map(|need| need.as_str()?.split_whitespace().next())
                                    .map(str::to_owned)
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default();
                        Some((name, needs))
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        loop {
            let grown = locked
                .iter()
                .filter(|(name, needs)| {
                    !packages.contains(name) && needs.iter().any(|need| packages.contains(need))
                })
                .map(|(name, _)| name.clone())
                .collect::<Vec<_>>();
            if grown.is_empty() {
                break;
            }
            packages.extend(grown);
        }
        // Of those, the runner's own dependencies: the harness the lock was
        // resolved for depends on all of them.
        packages.retain(|package| dependencies.iter().any(|(_, depended)| depended == package));
        let mut crates = dependencies
            .iter()
            .filter(|(_, package)| packages.contains(package))
            .map(|(key, _)| key.clone())
            .collect::<BTreeSet<_>>();
        crates.extend(
            hidden
                .iter()
                .filter(|found| packages.contains(&found.package))
                .map(|found| found.segment.clone()),
        );
        Some(Self {
            packages,
            crates,
            proxies: proxies.map(|path| format!("crate::{path}")).collect(),
            named: BTreeSet::new(),
        })
    }

    /// The program's packages, which the other members don't depend on.
    #[cfg(test)]
    fn packages(&self) -> &BTreeSet<String> {
        &self.packages
    }

    /// Its packages and the names the runner reaches them by.
    pub(super) fn into_crates(self) -> Crates {
        Crates {
            packages: self.packages,
            names: self.crates,
        }
    }

    /// Whether `name`, a crate the runner names, is one of the program's.
    pub(super) fn is_crate(&self, name: &str) -> bool {
        self.crates.contains(name)
    }

    /// Whether `text` names the program.
    pub(super) fn names(&self, text: &str) -> bool {
        identifiers(text).any(|word| self.crates.contains(word) || self.named.contains(word))
            || self.proxies.iter().any(|proxy| spells(text, proxy))
    }

    /// Which of `items`, each a group of item texts under one name, name
    /// the program, the items found to name it counted as the program's.
    /// Each group's name joins the program's when the group does.
    pub(super) fn sort(&mut self, groups: &[(Option<String>, Vec<&str>)]) -> Vec<bool> {
        let mut found = vec![false; groups.len()];
        loop {
            let mut grown = false;
            for (index, (name, texts)) in groups.iter().enumerate() {
                if found[index] || !texts.iter().any(|text| self.names(text)) {
                    continue;
                }
                found[index] = true;
                grown = true;
                if let Some(name) = name {
                    self.named.insert(name.clone());
                }
            }
            if !grown {
                return found;
            }
        }
    }
}

/// The program's crates once the runner's sources are split.
pub(super) struct Crates {
    /// Their packages, which only the program's member depends on.
    pub(super) packages: BTreeSet<String>,
    /// The names the runner's sources reach them by.
    pub(super) names: BTreeSet<String>,
}

/// Each `(key, package)` of the manifest's dependency tables, the
/// `target.<cfg>` ones' included.
fn dependencies(manifest: &DocumentMut) -> Vec<(String, String)> {
    let mut found = Vec::new();
    let mut take = |table: Option<&Item>| {
        let Some(table) = table.and_then(Item::as_table_like) else {
            return;
        };
        for (key, item) in table.iter() {
            let package = item
                .as_table_like()
                .and_then(|item| item.get("package"))
                .and_then(Item::as_str)
                .unwrap_or(key);
            found.push((key.to_owned(), package.to_owned()));
        }
    };
    take(manifest.get("dependencies"));
    if let Some(targets) = manifest.get("target").and_then(Item::as_table_like) {
        for (_, target) in targets.iter() {
            take(target.get("dependencies"));
        }
    }
    found
}

/// The identifiers of `text`, strings' and comments' among them.
fn identifiers(text: &str) -> impl Iterator<Item = &str> {
    text.split(|character: char| !(character.is_ascii_alphanumeric() || character == '_'))
        .filter(|word| !word.is_empty())
}

/// Whether `text` holds `path` ending where a path segment does.
fn spells(text: &str, path: &str) -> bool {
    text.match_indices(path).any(|(at, _)| {
        text[at + path.len()..]
            .chars()
            .next()
            .is_none_or(|character| !(character.is_ascii_alphanumeric() || character == '_'))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MANIFEST: &str = "[workspace]\n\n[package]\nname = \"rawr-generated-runner\"\n\n\
        [dependencies]\napp = { package = \"app-package\", path = \"/p/target/rawr/crate\" }\n\
        bin = { package = \"app-bin\", path = \"/p/target/rawr/bin\" }\nui = { path = \"/p/ui\" }\n";
    const LOCK: &str = "version = 4\n\n[[package]]\nname = \"app-bin\"\nversion = \"0.1.0\"\n\
        dependencies = [\n \"app-package\",\n \"ui\",\n]\n\n[[package]]\nname = \"app-package\"\n\
        version = \"0.1.0\"\ndependencies = [\n \"ui\",\n]\n\n[[package]]\nname = \"ui\"\nversion = \"0.1.0\"\n";

    /// The program is the script's crate and what depends on it; an item
    /// names it through one of its crates, a proxy, or an item that does.
    #[test]
    fn items_that_name_the_program_s_crates_proxies_or_items_are_the_program_s() {
        let mut program = Program::find(
            MANIFEST.as_bytes(),
            LOCK.as_bytes(),
            &[],
            "app",
            ["sidebar::Message".to_owned()].into_iter(),
        )
        .expect("the runner depends on the script's crate");
        assert_eq!(
            program
                .packages()
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>(),
            ["app-bin", "app-package"]
        );
        assert!(program.is_crate("bin"));
        assert!(!program.is_crate("ui"));
        let groups = [
            (
                Some("RawrFamily1".to_owned()),
                vec!["type Of<'x> = ui::Column<'x>;"],
            ),
            (
                Some("RawrFamily2".to_owned()),
                vec![
                    "pub struct RawrFamily2;",
                    "type Of<'x> = ui::Element<'x, app::Theme>;",
                ],
            ),
            (
                Some("NativePlace0".to_owned()),
                vec!["type Value = Vec<RawrFamily2>;"],
            ),
            (None, vec!["take::<crate::sidebar::Message>()"]),
            (None, vec!["take::<crate::sidebar::MessageList>()"]),
        ];
        assert_eq!(program.sort(&groups), [false, true, true, true, false]);
        assert!(program.names("adapter(NativePlace0)"));
        assert!(!program.names("adapter(ui::Column)"));
    }
}

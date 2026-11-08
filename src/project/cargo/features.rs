//! A package's features as Cargo resolves them: the feature flags a
//! command gives Cargo (`--features`, `--no-default-features` and
//! `--all-features`), and the features Cargo's unit graph enables for a
//! target from them. Rawr never reads them from `Cargo.toml` itself.

use std::ffi::OsString;
use std::process::Command;
use std::sync::{Arc, Mutex};

/// The feature flags of one `rawr run`, `rawr check` or `rawr test`,
/// exactly as written, for Cargo to read: `--features <list>` (or `-F`,
/// or `--features=<list>`), `--no-default-features` and `--all-features`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Selection {
    flags: Box<[OsString]>,
}

/// The features Cargo enables for one target: its unit's `features`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Enabled {
    /// Sorted.
    names: Arc<[Box<str>]>,
}

/// The variable that hands a selection to a `rawr` process `rawr` starts:
/// the program's own process, or the binary that serves the program.
const VARIABLE: &str = "RAWR_CARGO_FEATURES";

/// The selection this process runs with; read from [`VARIABLE`] when
/// nothing selected one.
static SELECTED: Mutex<Option<Selection>> = Mutex::new(None);

impl Selection {
    /// The feature flags among `arguments` before any `--`, and the
    /// arguments without them. A `--features` that ends the arguments
    /// is left for Cargo to refuse.
    #[must_use]
    pub fn take(arguments: Vec<OsString>) -> (Self, Vec<OsString>) {
        let end = arguments
            .iter()
            .position(|argument| argument == "--")
            .unwrap_or(arguments.len());
        let mut flags = Vec::new();
        let mut rest = Vec::with_capacity(arguments.len());
        let mut arguments = arguments.into_iter().enumerate();
        while let Some((index, argument)) = arguments.next() {
            if index >= end {
                rest.push(argument);
                continue;
            }
            let text = argument.to_string_lossy();
            let takes_list = text == "--features" || text == "-F";
            let flag = takes_list
                || text == "--no-default-features"
                || text == "--all-features"
                || text.starts_with("--features=");
            if !flag {
                rest.push(argument);
                continue;
            }
            flags.push(argument);
            if takes_list && index + 1 < end {
                flags.extend(arguments.next().map(|(_, list)| list));
            }
        }
        (
            Self {
                flags: flags.into_boxed_slice(),
            },
            rest,
        )
    }

    /// Make this the selection every Cargo answer of this process is asked
    /// with, unless it is empty and the process inherited one.
    pub fn select(self) {
        if self.flags.is_empty() && std::env::var_os(VARIABLE).is_some() {
            return;
        }
        *lock() = Some(self);
    }

    /// The selection this process runs with.
    #[must_use]
    pub fn selected() -> Self {
        lock().get_or_insert_with(Self::inherited).clone()
    }

    /// The flags, for Cargo.
    #[must_use]
    pub fn flags(&self) -> &[OsString] {
        &self.flags
    }

    /// Hand this selection to `command`, a `rawr` process.
    pub fn hand(&self, command: &mut Command) {
        if self.flags.is_empty() {
            return;
        }
        let mut encoded = OsString::new();
        for (index, flag) in self.flags.iter().enumerate() {
            if index > 0 {
                encoded.push("\n");
            }
            encoded.push(flag);
        }
        command.env(VARIABLE, encoded);
    }

    fn inherited() -> Self {
        let Some(encoded) = std::env::var_os(VARIABLE) else {
            return Self::default();
        };
        let flags = encoded
            .to_string_lossy()
            .split('\n')
            .filter(|flag| !flag.is_empty())
            .map(OsString::from)
            .collect();
        Self { flags }
    }
}

impl Enabled {
    /// The features of a unit as Cargo's unit graph lists them.
    pub(super) fn of_unit(unit: &serde_json::Value) -> Self {
        let mut names = unit["features"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|name| name.as_str().map(Box::from))
            .collect::<Vec<_>>();
        names.sort_unstable();
        names.dedup();
        Self {
            names: names.into(),
        }
    }

    /// Whether `cfg(feature = "<name>")` holds.
    #[must_use]
    pub fn holds(&self, name: &str) -> bool {
        self.names
            .binary_search_by(|enabled| (**enabled).cmp(name))
            .is_ok()
    }

    /// The enabled features' names, sorted.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.names.iter().map(AsRef::as_ref)
    }
}

fn lock() -> std::sync::MutexGuard<'static, Option<Selection>> {
    SELECTED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn arguments(words: &[&str]) -> Vec<OsString> {
        words.iter().map(OsString::from).collect()
    }

    #[test]
    fn feature_flags_before_the_program_arguments_are_cargo_s() {
        let (selection, rest) = Selection::take(arguments(&[
            "run",
            "--features",
            "a b",
            "app",
            "--no-default-features",
            "--",
            "--all-features",
        ]));
        assert_eq!(
            selection.flags(),
            arguments(&["--features", "a b", "--no-default-features"])
        );
        assert_eq!(rest, arguments(&["run", "app", "--", "--all-features"]));
        let (selection, _) = Selection::take(arguments(&["test", "-F", "x", "--features=y,z"]));
        assert_eq!(selection.flags(), arguments(&["-F", "x", "--features=y,z"]));
    }

    #[test]
    fn a_unit_s_features_hold_by_name() {
        let enabled = Enabled::of_unit(&serde_json::json!({ "features": ["x", "default"] }));
        assert!(enabled.holds("x") && enabled.holds("default"));
        assert!(!enabled.holds("y"));
        assert_eq!(enabled.names().collect::<Vec<_>>(), ["default", "x"]);
    }
}

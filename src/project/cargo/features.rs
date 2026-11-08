//! A package's features as Cargo resolves them: the feature flags a
//! command gives Cargo (`--features`, `--no-default-features` and
//! `--all-features`), and the features Cargo's unit graph enables for a
//! target from them. Rawr never reads them from `Cargo.toml` itself.
//! With them are the options rustc sets for the target (`super::cfg`):
//! together, the configuration a target's `cfg(..)`s read.

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

/// The features Cargo enables for one target, its unit's `features`, and
/// the options rustc sets for the target: what its `cfg(..)`s read.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Enabled {
    /// Sorted.
    names: Arc<[Box<str>]>,
    /// `None` for a source built outside any Cargo package, which has the
    /// host rustc's own (`Target::host`).
    target: Option<super::cfg::Target>,
}

/// The variable that hands a selection to a `rawr` process `rawr` starts:
/// the program's own process, or the binary that serves the program.
const VARIABLE: &str = "RAWR_CARGO_FEATURES";

/// How a feature starts as a line of [`Enabled::lines`].
const FEATURE: &str = "feature=\"";

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
        Self::of_names(
            unit["features"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(serde_json::Value::as_str),
        )
    }

    /// The features named: a set Cargo resolved, handed on to a runner in
    /// its snapshot.
    #[must_use]
    pub fn of_names<'a>(names: impl IntoIterator<Item = &'a str>) -> Self {
        let mut names = names.into_iter().map(Box::from).collect::<Vec<_>>();
        names.sort_unstable();
        names.dedup();
        Self {
            names: names.into(),
            target: None,
        }
    }

    /// The set with `target`'s options: the ones rustc sets for the target
    /// and profile Cargo builds the unit with.
    #[must_use]
    pub(super) fn with_target(self, target: super::cfg::Target) -> Self {
        Self {
            target: Some(target),
            ..self
        }
    }

    /// The set rustc's lines give, as [`Enabled::lines`] writes them: a
    /// runner's, handed on in its snapshot.
    #[must_use]
    pub fn of_lines<'a>(lines: impl IntoIterator<Item = &'a str>) -> Self {
        let (features, options): (Vec<_>, Vec<_>) = lines
            .into_iter()
            .partition(|line| line.starts_with(FEATURE));
        Self::of_names(features.into_iter().map(|line| {
            let name = &line[FEATURE.len()..];
            name.strip_suffix('"').unwrap_or(name)
        }))
        .with_target(super::cfg::Target::of_lines(options))
    }

    /// The whole set as rustc's `--print cfg` writes it, one option to a
    /// line, each feature as `feature="<name>"`.
    pub fn lines(&self) -> impl Iterator<Item = String> + '_ {
        self.names
            .iter()
            .map(|name| format!("{FEATURE}{name}\""))
            .chain(self.target().lines().map(str::to_owned))
    }

    /// Whether `cfg(<name>)`, or `cfg(<name> = "<value>")`, holds for an
    /// option rustc sets: `unix`, `target_os = ".."`, `debug_assertions`.
    #[must_use]
    pub fn option(&self, name: &str, value: Option<&str>) -> bool {
        self.target().holds(name, value)
    }

    fn target(&self) -> &super::cfg::Target {
        self.target
            .as_ref()
            .unwrap_or_else(|| super::cfg::Target::host())
    }

    /// Whether the `cfg` option `name` holds, `name = "<value>"` with
    /// `value`: `feature = ".."` for a feature Cargo enables, `test` when
    /// the crate builds as rustc's `--test` builds it, and any other as
    /// rustc sets it for the target. Every reader of a `cfg(..)` asks this.
    #[must_use]
    pub fn cfg(&self, name: &str, value: Option<&str>, test: bool) -> bool {
        match (name, value) {
            ("feature", Some(feature)) => self.holds(feature),
            ("test", None) => test,
            (name, value) => self.option(name, value),
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

/// A `cfg` operator over the predicates in its parentheses.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Operator {
    /// `not(..)`: its one predicate does not hold.
    Not,
    /// `all(..)`: every predicate holds.
    All,
    /// `any(..)`: some predicate holds.
    Any,
}

impl Operator {
    /// The operator `name` names: `not`, `all` or `any`.
    #[must_use]
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "not" => Some(Self::Not),
            "all" => Some(Self::All),
            "any" => Some(Self::Any),
            _ => None,
        }
    }

    /// Whether the operator holds over its predicates' `values`.
    #[must_use]
    pub fn holds(self, values: &[bool]) -> bool {
        match self {
            Self::Not => !values.first().copied().unwrap_or(false),
            Self::All => values.iter().all(|value| *value),
            Self::Any => values.iter().any(|value| *value),
        }
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

    #[test]
    fn a_set_s_lines_give_the_set_back() {
        let target = super::super::cfg::Target::of_lines(["unix", "target_os=\"macos\""]);
        let enabled = Enabled::of_names(["x"]).with_target(target);
        let lines = enabled.lines().collect::<Vec<_>>();
        assert_eq!(lines, ["feature=\"x\"", "target_os=\"macos\"", "unix"]);
        let back = Enabled::of_lines(lines.iter().map(String::as_str));
        assert_eq!(back, enabled);
        assert!(back.holds("x") && back.option("unix", None) && !back.holds("unix"));
    }
}

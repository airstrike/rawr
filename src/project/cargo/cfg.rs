//! The configuration options rustc sets for a target: what `rustc --print
//! cfg` prints for the target and profile Cargo builds a unit with
//! (`target_os = ".."`, `unix`, `debug_assertions`, ..). Rawr keeps no
//! table of them; rustc is asked once per answer Cargo gives.

use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::{Arc, OnceLock};

/// One target's options, as `rustc --print cfg` prints them.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct Target {
    /// Each option's line, sorted: `unix`, or `target_os="macos"`.
    options: Arc<[Box<str>]>,
}

/// rustc could not print a target's options.
#[derive(Debug)]
pub(super) enum Error {
    /// rustc could not be started.
    Spawn(std::io::Error),
    /// rustc refused, with what it printed on stderr.
    Failed(Box<str>),
}

impl Target {
    /// The options rustc sets for a unit of Cargo's unit graph: for its
    /// `platform` (the host when it names none) and its profile's
    /// `debug_assertions` and `overflow_checks`. Asked from `directory`,
    /// where rustup picks the toolchain Cargo builds the package with.
    pub(super) fn of_unit(directory: &Path, unit: &serde_json::Value) -> Result<Self, Error> {
        let mut command = rustc();
        command.current_dir(directory).args(["--print", "cfg"]);
        if let Some(platform) = unit["platform"].as_str() {
            command.args(["--target", platform]);
        }
        for (flag, setting) in [
            ("debug-assertions", "debug_assertions"),
            ("overflow-checks", "overflow_checks"),
        ] {
            let on = unit["profile"][setting].as_bool().unwrap_or(false);
            command.arg(format!("-C{flag}={}", if on { "on" } else { "off" }));
        }
        let output = command.output().map_err(Error::Spawn)?;
        if !output.status.success() {
            return Err(Error::Failed(
                String::from_utf8_lossy(&output.stderr).into(),
            ));
        }
        Ok(Self::of_lines(
            String::from_utf8_lossy(&output.stdout).lines(),
        ))
    }

    /// The options of a source built outside any Cargo package, as `rustc
    /// file.rs` builds it on this host: rustc's own defaults. Asked once
    /// per process; none when rustc cannot be asked.
    #[must_use]
    pub fn host() -> &'static Self {
        static HOST: OnceLock<Target> = OnceLock::new();
        HOST.get_or_init(|| {
            let output = rustc().args(["--print", "cfg"]).output();
            match output {
                Ok(output) if output.status.success() => {
                    Self::of_lines(String::from_utf8_lossy(&output.stdout).lines())
                }
                _ => Self::default(),
            }
        })
    }

    /// The options written as rustc prints them, one to a line: `name` or
    /// `name="value"`.
    pub fn of_lines<'a>(lines: impl IntoIterator<Item = &'a str>) -> Self {
        let mut options = lines
            .into_iter()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(Box::from)
            .collect::<Vec<_>>();
        options.sort_unstable();
        options.dedup();
        Self {
            options: options.into(),
        }
    }

    /// Whether the option `name`, with `value` when it has one, is set:
    /// `cfg(unix)` or `cfg(target_os = "macos")`.
    #[must_use]
    pub fn holds(&self, name: &str, value: Option<&str>) -> bool {
        let line = match value {
            Some(value) => format!("{name}=\"{value}\""),
            None => name.to_owned(),
        };
        self.options
            .binary_search_by(|set| (**set).cmp(line.as_str()))
            .is_ok()
    }

    /// The options as rustc prints them, sorted.
    pub fn lines(&self) -> impl Iterator<Item = &str> {
        self.options.iter().map(AsRef::as_ref)
    }
}

/// `$RUSTC`, else `rustc`, as Cargo picks its compiler, with no stdin.
fn rustc() -> Command {
    let program = std::env::var_os("RUSTC")
        .filter(|rustc| !rustc.is_empty())
        .unwrap_or_else(|| "rustc".into());
    let mut command = Command::new(program);
    command.stdin(Stdio::null());
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn options_hold_by_name_and_value_as_rustc_prints_them() {
        let target = Target::of_lines(["unix", "target_os=\"macos\"", "debug_assertions", ""]);
        assert!(target.holds("unix", None) && target.holds("target_os", Some("macos")));
        assert!(!target.holds("target_os", None) && !target.holds("windows", None));
        assert!(!target.holds("target_os", Some("linux")));
        assert_eq!(
            target.lines().collect::<Vec<_>>(),
            ["debug_assertions", "target_os=\"macos\"", "unix"]
        );
    }
}

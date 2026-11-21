//! What `start`, `run` and `check` take beside their path, as `rawr run`
//! takes it (#2398): a workspace's `package` and a package's `bin`, which
//! choose the program's path as `-p` and `--bin` choose it; the feature
//! flags, which the child `rawr` hands every Cargo answer of the run; and
//! `env`, which the child process starts with, so the build and the program
//! read it. The child keeps all of them for its reloads.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Command;

use serde_json::Value;

/// One call's options.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Options {
    /// The workspace package, as `-p`.
    package: Option<String>,
    /// The package's binary, as `--bin`.
    bin: Option<String>,
    /// The features, as `--features`.
    features: Vec<String>,
    /// `--no-default-features`.
    no_default_features: bool,
    /// `--all-features`.
    all_features: bool,
    /// Variables set in the child's environment, over the server's.
    environment: BTreeMap<String, String>,
}

impl Options {
    /// The options among a call's `arguments`, or why one is malformed.
    pub fn new(arguments: &Value) -> Result<Self, String> {
        let text = |key: &str| -> Result<Option<String>, String> {
            match arguments.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(text)) => Ok(Some(text.clone())),
                Some(_) => Err(format!("`{key}` is a string")),
            }
        };
        let flag = |key: &str| -> Result<bool, String> {
            match arguments.get(key) {
                None | Some(Value::Null) => Ok(false),
                Some(Value::Bool(set)) => Ok(*set),
                Some(_) => Err(format!("`{key}` is a boolean")),
            }
        };
        let features = match arguments.get("features") {
            None | Some(Value::Null) => Vec::new(),
            Some(Value::Array(items)) => items
                .iter()
                .map(|item| item.as_str().map(str::to_owned))
                .collect::<Option<_>>()
                .ok_or("`features` is an array of strings")?,
            Some(_) => return Err("`features` is an array of strings".to_owned()),
        };
        let environment = match arguments.get("env") {
            None | Some(Value::Null) => BTreeMap::new(),
            Some(Value::Object(variables)) => variables
                .iter()
                .map(|(name, value)| value.as_str().map(|value| (name.clone(), value.to_owned())))
                .collect::<Option<_>>()
                .ok_or("`env` maps each variable's name to a string")?,
            Some(_) => return Err("`env` maps each variable's name to a string".to_owned()),
        };
        Ok(Self {
            package: text("package")?,
            bin: text("bin")?,
            features,
            no_default_features: flag("no_default_features")?,
            all_features: flag("all_features")?,
            environment,
        })
    }

    /// The program's path: `path`, or the directory of its workspace's
    /// `package`, then the crate root of that package's `bin`, as `rawr run
    /// -p <package> --bin <bin> <path>` puts them in its place.
    pub fn program(
        &self,
        path: PathBuf,
        answers: &mut rawr::project::cargo::Answers,
    ) -> Result<PathBuf, String> {
        let path = match &self.package {
            Some(name) => rawr_location::package(&path, name).ok_or_else(|| {
                format!("no package `{name}` in the workspace at {}", path.display())
            })?,
            None => path,
        };
        let Some(name) = &self.bin else {
            return Ok(path);
        };
        let start = if path.is_dir() {
            path.as_path()
        } else {
            path.parent().unwrap_or(&path)
        };
        rawr::project::Target::binary(start, name, answers).map_err(|error| error.to_string())
    }

    /// The feature flags, as `rawr run` takes them.
    #[must_use]
    pub fn flags(&self) -> Vec<String> {
        let mut flags = Vec::new();
        if !self.features.is_empty() {
            flags.push("--features".to_owned());
            flags.push(self.features.join(","));
        }
        if self.no_default_features {
            flags.push("--no-default-features".to_owned());
        }
        if self.all_features {
            flags.push("--all-features".to_owned());
        }
        flags
    }

    /// The variables set over the server's environment.
    #[must_use]
    pub fn environment(&self) -> &BTreeMap<String, String> {
        &self.environment
    }

    /// Give `command`, a `rawr run` or `rawr check` whose path is already
    /// in, the feature flags and the environment.
    pub fn apply(&self, command: &mut Command) {
        command.args(self.flags()).envs(&self.environment);
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::Options;

    #[test]
    fn options_parse_as_rawr_run_flags() {
        let options = Options::new(&json!({
            "path": "/w",
            "package": "app",
            "bin": "tool",
            "features": ["ui", "extra"],
            "no_default_features": true,
            "all_features": false,
            "env": { "CARGO_TARGET_DIR": "/t", "DATA": "x" },
        }))
        .unwrap();
        assert_eq!(options.package.as_deref(), Some("app"));
        assert_eq!(options.bin.as_deref(), Some("tool"));
        assert_eq!(
            options.flags(),
            ["--features", "ui,extra", "--no-default-features"]
        );
        assert_eq!(options.environment()["DATA"], "x");
        assert_eq!(
            Options::new(&json!({ "path": "/w" })).unwrap(),
            Options::default()
        );
        assert!(Options::default().flags().is_empty());
        assert_eq!(
            Options::new(&json!({ "all_features": true }))
                .unwrap()
                .flags(),
            ["--all-features"]
        );
    }

    #[test]
    fn malformed_options_are_refused() {
        for arguments in [
            json!({ "features": "ui" }),
            json!({ "features": [1] }),
            json!({ "env": { "A": 1 } }),
            json!({ "env": ["A=1"] }),
            json!({ "bin": true }),
            json!({ "all_features": "yes" }),
        ] {
            assert!(Options::new(&arguments).is_err(), "{arguments}");
        }
    }
}

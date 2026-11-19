//! The crates a package's paths may start with, as rustc's extern prelude
//! has them: `std`, `core` and each dependency its manifest names. A lone
//! file names none, so `#[tokio::main]` and `tokio::join!` in it are
//! rustc's E0433, though Rawr reads both itself where tokio is a
//! dependency. A call of a dependency's function-like macro, which
//! rustc expands, finds its expansion in the project the package is
//! built in (#1125).

use std::path::{Path, PathBuf};

use crate::package::graph::Package;
use crate::{ImmutableString, Span};

use super::Error;

/// The crates always in the extern prelude.
const ALWAYS: [&str; 2] = ["std", "core"];

/// The extern prelude of one package.
#[derive(Clone, Debug)]
pub(crate) struct Prelude {
    dependencies: Box<[ImmutableString]>,
    /// The project the package is built in, whose `target/rawr/expand`
    /// keeps rustc's expansions.
    #[cfg_attr(not(feature = "rust-import"), allow(dead_code))]
    project: PathBuf,
}

impl Prelude {
    /// The extern prelude of `package`: its Rust dependencies, by the names
    /// its code spells them (`async-std` is `async_std`), in the project at
    /// `project`.
    pub(crate) fn of(package: &Package, project: &Path) -> Self {
        let dependencies = package
            .rust
            .as_ref()
            .map(|config| {
                config
                    .dependencies
                    .iter()
                    .map(|dependency| dependency.alias.replace('-', "_").into())
                    .collect()
            })
            .unwrap_or_default();
        Self {
            dependencies,
            project: project.to_path_buf(),
        }
    }

    /// Whether a dependency of the package is named `name`, which std and
    /// core are not.
    pub(crate) fn dependency(&self, name: &str) -> bool {
        self.dependencies
            .iter()
            .any(|dependency| dependency.as_str() == name)
    }

    /// The project the package is built in.
    #[cfg_attr(not(feature = "rust-import"), allow(dead_code))]
    pub(crate) fn project(&self) -> &Path {
        &self.project
    }

    /// Whether `path` names a macro of one of the package's dependencies
    /// that `host`'s facades hold no item at: a proc macro, whose crate has
    /// no API to extract, or one a library re-exports from its proc macro
    /// crate. rustc resolves the path when it expands the call, and names
    /// it in its error when it holds no macro there. A macro is a module's
    /// item, so a path below a function or a type the facades hold names
    /// none: `dep::function::name` is an unresolved import.
    pub(crate) fn crate_macro(&self, host: &crate::host::Registry, path: &str) -> bool {
        path.split_once("::")
            .is_some_and(|(root, _)| self.dependency(root))
            && path.match_indices("::").skip(1).all(|(end, _)| {
                matches!(
                    host.entry(&path[..end]),
                    None | Some(crate::host::Entry::Module | crate::host::Entry::ModuleItem(_))
                )
            })
            && !host
                .overloads(path)
                .is_some_and(|items| items.iter().any(|item| item.kind().is_macro()))
    }

    /// Whether it holds a crate named `name`.
    pub(crate) fn holds(&self, name: &str) -> bool {
        ALWAYS.contains(&name) || self.dependency(name)
    }

    /// `name`, a path's first segment at `at`, as a crate this prelude
    /// holds: rustc's E0433 when it holds none of that name.
    pub(crate) fn reach(&self, name: &str, at: Span) -> Result<(), Error> {
        if self.holds(name) {
            return Ok(());
        }
        Err(Error::UnknownType {
            name: name.into(),
            reference: at,
            path: true,
        })
    }
}

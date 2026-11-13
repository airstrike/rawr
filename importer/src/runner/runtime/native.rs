//! The static libraries a build's scripts link, with the package that links
//! each.
//!
//! A runner that links rawr's shared runtime holds two compiled graphs: the
//! project's and the runtime's. A package both name compiles twice, and its
//! Rust symbols are mangled apart. A static library carries unmangled
//! symbols, so when both graphs link one, the linker binds every caller to
//! one copy. rawr shares that copy when both come from one package at
//! versions Cargo treats as compatible (D831). Cargo would build one version
//! for both, but here one side's Rust code may call the other version's native
//! code: that rests on the crate keeping its native interface stable within a
//! compatible range, not on a guarantee from Cargo. Any other pair of copies
//! may disagree on what a symbol does, and the runner compiles rawr in the
//! project.

use std::collections::BTreeSet;

/// One static library, as a build script links it.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Link {
    library: String,
    /// `None` for a record that names no package, which shares with nothing.
    package: Option<Package>,
}

/// The package whose build script links a library.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct Package {
    name: String,
    source: String,
    version: semver::Version,
}

impl Link {
    /// Every static library the build scripts in cargo's JSON `messages`
    /// link.
    pub(crate) fn all_in(messages: &crate::cargo::messages::Messages<'_>) -> BTreeSet<Self> {
        let mut links = BTreeSet::new();
        for message in messages.iter() {
            if message.reason() != Some("build-script-executed") {
                continue;
            }
            let package = message.package_id().and_then(Package::parse);
            for library in message.linked_libs() {
                if let Some((kind, name)) = library.split_once('=')
                    && kind.starts_with("static")
                {
                    links.insert(Self {
                        library: name.split(':').next().unwrap_or(name).to_owned(),
                        package: package.clone(),
                    });
                }
            }
        }
        links
    }

    /// The link a line of [`Link::record`] names. A line from before the
    /// record named packages gives a link that shares with nothing.
    pub(crate) fn from_record(line: &str) -> Option<Self> {
        let (library, package) = match line.split_once('\t') {
            Some((library, package)) => (library, Package::parse(package)),
            None => (line, None),
        };
        (!library.is_empty()).then(|| Self {
            library: library.to_owned(),
            package,
        })
    }

    /// One line of the runtime's record of what it links.
    pub(crate) fn record(&self) -> String {
        match &self.package {
            Some(package) => format!(
                "{}\t{}#{}@{}\n",
                self.library, package.source, package.name, package.version
            ),
            None => format!("{}\n", self.library),
        }
    }

    /// Why `self`, which the project links, can't share a runner with
    /// `runtime`, which rawr's shared runtime links: the same library from
    /// two packages, or from one package at versions Cargo keeps apart.
    /// `None` when they share, or name different libraries.
    pub(crate) fn conflict(&self, runtime: &Self) -> Option<String> {
        if self.library != runtime.library {
            return None;
        }
        match (&self.package, &runtime.package) {
            (Some(project), Some(shared)) if project.shares_with(shared) => None,
            (Some(project), Some(shared)) => Some(format!(
                "the native library `{}` is linked both by this project's {} {} and by rawr's \
                 shared runtime's {} {}, which Cargo would not resolve to one version",
                self.library, project.name, project.version, shared.name, shared.version
            )),
            _ => Some(format!(
                "the native library `{}` is linked both by this project and by rawr's shared \
                 runtime",
                self.library
            )),
        }
    }
}

impl Package {
    /// A package from Cargo's package id: `source#name@version`,
    /// `source#version` for a package named as its directory, or the older
    /// `name version (source)`.
    fn parse(id: &str) -> Option<Self> {
        let (name, version, source) = match id.split_once('#') {
            Some((source, rest)) => {
                let (name, version) = match rest.split_once('@') {
                    Some((name, version)) => (name, version),
                    None => {
                        let path = source.split(['?', '#']).next().unwrap_or(source);
                        (path.trim_end_matches('/').rsplit('/').next()?, rest)
                    }
                };
                (name, version, source)
            }
            None => {
                let mut parts = id.splitn(3, ' ');
                let name = parts.next()?;
                let version = parts.next()?;
                let source = parts.next()?.strip_prefix('(')?.strip_suffix(')')?;
                (name, version, source)
            }
        };
        Some(Self {
            name: name.to_owned(),
            source: source.to_owned(),
            version: semver::Version::parse(version).ok()?,
        })
    }

    /// Whether one Cargo graph would resolve `self` and `other` to a single
    /// version: one package from one source, with versions a caret
    /// requirement on either accepts. A pre-release shares only with itself.
    fn shares_with(&self, other: &Self) -> bool {
        if self.name != other.name || self.source != other.source {
            return false;
        }
        let (mine, theirs) = (&self.version, &other.version);
        if !mine.pre.is_empty() || !theirs.pre.is_empty() {
            return mine == theirs;
        }
        match (mine.major, mine.minor) {
            (0, 0) => theirs.major == 0 && theirs.minor == 0 && theirs.patch == mine.patch,
            (0, minor) => theirs.major == 0 && theirs.minor == minor,
            (major, _) => theirs.major == major,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Link;

    const REGISTRY: &str = "registry+https://github.com/rust-lang/crates.io-index";

    fn executed(package_id: &str, libraries: &[&str]) -> String {
        let libraries = libraries
            .iter()
            .map(|library| format!("{library:?}"))
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"reason\":\"build-script-executed\",\"package_id\":{package_id:?},\
             \"linked_libs\":[{libraries}],\"linked_paths\":[],\"cfgs\":[],\"env\":[],\
             \"out_dir\":\"/o\"}}\n"
        )
    }

    fn only(messages: &str) -> Link {
        let mut links = Link::all_in(&crate::cargo::messages::Messages::parse(
            messages.as_bytes(),
        ))
        .into_iter();
        let link = links.next().expect("the messages link one static library");
        assert_eq!(links.next(), None, "the messages link one static library");
        link
    }

    #[test]
    fn a_native_library_from_one_package_at_compatible_versions_is_shared() {
        let runtime = only(&executed(
            &format!("{REGISTRY}#psm@0.1.31"),
            &["static=psm_s", "dylib=System"],
        ));
        let newer = only(&executed(
            &format!("{REGISTRY}#psm@0.1.32"),
            &["static=psm_s"],
        ));
        let older = only(&executed(
            &format!("psm 0.1.21 ({REGISTRY})"),
            &["static:+whole-archive=psm_s"],
        ));
        assert_eq!(newer.conflict(&runtime), None);
        assert_eq!(older.conflict(&runtime), None);
        let recorded = Link::from_record(runtime.record().trim_end()).unwrap();
        assert_eq!(recorded, runtime);
        assert_eq!(newer.conflict(&recorded), None);
    }

    #[test]
    fn a_native_library_cargo_would_keep_apart_is_a_conflict() {
        let runtime = only(&executed(
            &format!("{REGISTRY}#psm@0.1.31"),
            &["static=psm_s"],
        ));
        let incompatible = only(&executed(
            &format!("{REGISTRY}#psm@0.2.0"),
            &["static=psm_s"],
        ));
        let reason = incompatible.conflict(&runtime).unwrap();
        assert!(
            reason.contains("psm 0.2.0") && reason.contains("psm 0.1.31"),
            "{reason}"
        );
        let other = only(&executed(
            "path+file:///work/vendored-psm#0.1.31",
            &["static=psm_s"],
        ));
        assert!(other.conflict(&runtime).is_some());
        let unnamed = Link::from_record("psm_s").unwrap();
        assert!(unnamed.conflict(&runtime).is_some());
        let unrelated = only(&executed(
            &format!("{REGISTRY}#zstd-sys@2.0.0"),
            &["static=zstd"],
        ));
        assert_eq!(unrelated.conflict(&runtime), None);
    }
}

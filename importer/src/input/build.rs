//! Bounded public Cargo Build messages.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use super::package;
use super::Usage;
use crate::cargo::unit;

pub(in crate::input) mod packages;
pub(in crate::input) mod platform;
pub(in crate::input) mod scripts;

/// Bounded public Cargo JSON evidence from the completed normal Build command.
pub(crate) struct Build {
    pub(super) evidence: Evidence,
    pub(super) package_files: BTreeMap<Box<str>, package::Set>,
}

#[cfg(feature = "rustc-driver")]
impl Build {
    /// The extraction toolchain's observed build-script outputs. A package
    /// whose generated files cannot be read has no shareable evidence.
    pub(crate) fn binding_scripts(&self) -> BTreeMap<String, Option<Vec<crate::bindings::Script>>> {
        let mut results = BTreeMap::<String, Option<Vec<crate::bindings::Script>>>::new();
        for script in &self.evidence.scripts {
            let output = crate::bindings::Script::new(
                script.cfg.iter().map(ToString::to_string),
                script
                    .environment
                    .iter()
                    .map(|(name, value)| (name.to_string(), value.to_string())),
                &script.out_dir,
            );
            let kept = results
                .entry(script.package_id.to_string())
                .or_insert_with(|| Some(Vec::new()));
            match output {
                Ok(output) => {
                    if let Some(outputs) = kept
                        && !outputs.contains(&output)
                    {
                        outputs.push(output);
                        outputs.sort_unstable();
                    }
                }
                Err(_) => *kept = None,
            }
        }
        results
    }
}

/// Historical Build observations after package-file ownership moved to a baseline.
/// The borrowed view cannot be consumed into another baseline capture.
pub(crate) struct Retained {
    pub(super) build: Build,
}

impl Retained {
    pub(crate) fn build(&self) -> &Build {
        &self.build
    }

    /// Public Build output paths, without interpreting Cargo's scratch layout.
    #[cfg(feature = "rustc-driver")]
    pub(crate) fn artifact_outputs(&self) -> impl Iterator<Item = &std::path::Path> {
        self.build
            .evidence
            .artifacts
            .iter()
            .flat_map(|artifact| artifact.outputs.iter().map(PathBuf::as_path))
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) fn dep_info_paths(&self) -> impl Iterator<Item = &std::path::Path> {
        self.build
            .evidence
            .artifacts
            .iter()
            .map(|artifact| artifact.dep_info.as_path())
    }

    #[cfg(feature = "rustc-driver")]
    pub(crate) fn out_directories(&self) -> impl Iterator<Item = &std::path::Path> {
        self.build
            .evidence
            .scripts
            .iter()
            .map(|script| script.out_dir.as_path())
    }
}

#[cfg(test)]
impl Build {
    pub(crate) fn fixture_storage(&self) -> [usize; 5] {
        [
            self.evidence.artifacts.as_ptr() as usize,
            self.evidence.scripts.as_ptr() as usize,
            self.evidence.environment.as_ptr() as usize,
            self.evidence
                .packages
                .first_key_value()
                .map_or(0, |(key, _)| key.as_ptr() as usize),
            self.evidence
                .artifacts
                .first()
                .map_or(0, |artifact| artifact.outputs.as_ptr() as usize),
        ]
    }
}

pub(super) struct Evidence {
    pub(super) usage: Usage,
    pub(super) packages: packages::Table,
    pub(super) scripts: Box<[Script]>,
    pub(super) artifacts: Box<[Artifact]>,
    pub(super) environment: Box<[(OsString, OsString)]>,
}

#[derive(Clone)]
pub(super) struct Package {
    pub(super) manifest: PathBuf,
    pub(super) root: PathBuf,
    pub(super) build_script: Option<PathBuf>,
}

pub(super) struct Script {
    /// Derived from the current authenticated graph; never serialized.
    pub(super) compiler: Option<(usize, bool)>,
    pub(super) package_id: Box<str>,
    pub(super) out_dir: PathBuf,
    pub(super) cfg: Box<[Box<str>]>,
    pub(super) environment: Box<[(Box<str>, Box<str>)]>,
}

pub(super) struct Artifact {
    pub(super) package_id: Box<str>,
    pub(super) outputs: Box<[PathBuf]>,
    pub(super) dep_info: PathBuf,
    pub(super) key: Key,
    pub(super) profile: unit::ArtifactProfile,
    pub(super) fresh: bool,
}

impl Artifact {
    pub(super) fn same_public_message(&self, other: &Self) -> bool {
        self.package_id == other.package_id
            && self.outputs == other.outputs
            && self.dep_info == other.dep_info
            && self.key == other.key
            && self.profile == other.profile
            && self.fresh == other.fresh
    }
}

#[derive(Eq, Ord, PartialEq, PartialOrd)]
pub(super) struct Key {
    pub(super) package_id: Box<str>,
    pub(super) target: PathBuf,
    pub(super) target_kinds: Box<[Box<str>]>,
    pub(super) crate_types: Box<[Box<str>]>,
    pub(super) crate_name: Box<str>,
    pub(super) features: Box<[Box<str>]>,
}

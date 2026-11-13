//! The units one Cargo build named, read from its JSON messages.
//!
//! Every file of a unit in a build directory carries the unit's 16-hex hash
//! (`deps/libserde-<hash>.rlib`, `.fingerprint/serde-<hash>`,
//! `build/serde-<hash>/out`), so a unit is its hash. A message that names
//! only an uplifted copy (a binary, which Cargo copies out of `deps`) is
//! matched to the hashed file of its name and bytes.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::bridge::generate::member;

/// The units a build named, by hash.
#[derive(Default)]
pub(crate) struct Named {
    units: BTreeMap<String, Unit>,
}

/// One unit a build named.
pub(crate) struct Unit {
    /// Its crate, as `deps` spells it.
    krate: String,
    /// Whether this build compiled it.
    rebuilt: bool,
    /// Whether its package is one rawr generates: the runner's, a member's,
    /// or one under rawr's own directory.
    generated: bool,
    /// Whether it is a crate rustc compiles, with an incremental directory
    /// of its crate's name: not a build script or its run.
    crate_target: bool,
}

impl Named {
    /// The units `messages` name, whose hashed files lie in `profiles`.
    /// `rawr` is the directory rawr generates packages under, if any.
    pub(crate) fn read(
        messages: &crate::cargo::messages::Messages<'_>,
        profiles: &[PathBuf],
        rawr: Option<&Path>,
    ) -> Self {
        let mut named = Self::default();
        let mut hashed = None;
        for message in messages.iter() {
            let generated = message
                .package_id()
                .and_then(super::package_name)
                .is_some_and(member::is_runner_package)
                || message
                    .manifest_path()
                    .zip(rawr)
                    .is_some_and(|(manifest, rawr)| Path::new(manifest).starts_with(rawr));
            let crate_target = message.reason() == Some("compiler-artifact")
                && message.target_kind() != Some("custom-build");
            let rebuilt = message.fresh() == Some(false);
            for path in message.paths() {
                let hashed = &mut hashed;
                let Some((krate, hash)) = unit_of(Path::new(path), move || {
                    hashed.get_or_insert_with(|| listing(profiles)).as_slice()
                }) else {
                    continue;
                };
                let unit = named.units.entry(hash).or_insert(Unit {
                    krate,
                    rebuilt: false,
                    generated: false,
                    crate_target: false,
                });
                unit.crate_target |= crate_target;
                unit.rebuilt |= rebuilt;
                unit.generated |= generated;
            }
        }
        named
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.units.is_empty()
    }

    /// Each unit's hash and what the build said of it.
    pub(crate) fn iter(&self) -> impl Iterator<Item = (&str, &Unit)> {
        self.units.iter().map(|(hash, unit)| (hash.as_str(), unit))
    }

    /// The hashes, in order.
    pub(crate) fn hashes(&self) -> impl Iterator<Item = &str> {
        self.units.keys().map(String::as_str)
    }
}

impl Unit {
    pub(crate) fn krate(&self) -> &str {
        &self.krate
    }

    pub(crate) const fn rebuilt(&self) -> bool {
        self.rebuilt
    }

    pub(crate) const fn generated(&self) -> bool {
        self.generated
    }

    pub(crate) const fn crate_target(&self) -> bool {
        self.crate_target
    }
}

/// The unit `path` belongs to: by its hashed name or its directory's, or,
/// for an uplifted copy, the hashed file in `deps` of its name and bytes.
fn unit_of<'listing>(
    path: &Path,
    hashed: impl FnOnce() -> &'listing [PathBuf],
) -> Option<(String, String)> {
    let named = [path.file_name(), path.parent().and_then(Path::file_name)]
        .into_iter()
        .flatten()
        .find_map(|part| part.to_str().and_then(super::unit_key));
    if let Some((krate, hash)) = named {
        return Some((krate, hash.to_owned()));
    }
    let name = path.file_name()?.to_str()?.replace('-', "_");
    let length = fs::metadata(path).ok()?.len();
    // Of one name and length there is almost always one; only then do the
    // bytes need comparing.
    let mut alike = Vec::new();
    for candidate in hashed() {
        let Some(file) = candidate.file_name().and_then(|file| file.to_str()) else {
            continue;
        };
        let Some((krate, hash)) = super::unit_key(file) else {
            continue;
        };
        if unhashed(file, hash) == name
            && fs::metadata(candidate).is_ok_and(|original| original.len() == length)
        {
            alike.push((candidate, krate, hash));
        }
    }
    if alike.len() > 1 {
        let bytes = fs::read(path).ok()?;
        alike.retain(|(candidate, _, _)| fs::read(candidate).is_ok_and(|other| other == bytes));
    }
    let [(_, krate, hash)] = <[_; 1]>::try_from(alike).ok()?;
    Some((krate, hash.to_owned()))
}

/// The hashed files in each profile's `deps`.
fn listing(profiles: &[PathBuf]) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for profile in profiles {
        let entries = fs::read_dir(profile.join("deps"))
            .into_iter()
            .flatten()
            .flatten();
        files.extend(entries.map(|entry| entry.path()).filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .and_then(super::unit_key)
                .is_some()
        }));
    }
    files
}

/// `file` with its `-<hash>` taken out and dashes as underscores, as Cargo
/// names the uplifted copy.
fn unhashed(file: &str, hash: &str) -> String {
    file.replacen(&format!("-{hash}"), "", 1).replace('-', "_")
}

#[cfg(test)]
mod tests {
    use std::fs;

    #[test]
    fn a_build_names_its_units_by_hash_and_an_uplifted_copy_by_its_original() {
        let project = crate::store::TestProject::new("rawr-deps-named");
        let profile = project.path().join("build/debug");
        let deps = profile.join("deps");
        fs::create_dir_all(&deps).unwrap();
        let hash = |digit: char| std::iter::repeat_n(digit, 16).collect::<String>();
        let rlib = deps.join(format!("libserde-{}.rlib", hash('a')));
        fs::write(&rlib, b"rlib").unwrap();
        let binary = deps.join(format!("rawr_generated_runner-{}", hash('b')));
        fs::write(&binary, b"runner").unwrap();
        fs::write(
            deps.join(format!("rawr_generated_runner-{}", hash('c'))),
            b"older",
        )
        .unwrap();
        let artifacts = project.path().join("artifacts");
        fs::create_dir_all(&artifacts).unwrap();
        let copy = artifacts.join("rawr-generated-runner");
        fs::write(&copy, b"runner").unwrap();
        let out = profile.join(format!("build/serde-{}/out", hash('d')));
        let messages = [
            format!(
                r#"{{"reason":"compiler-artifact","package_id":"registry+https://x#serde@1.0.0","fresh":true,"filenames":["{}"]}}"#,
                rlib.display()
            ),
            format!(
                r#"{{"reason":"compiler-artifact","package_id":"path+file:///r#rawr-generated-runner@0.0.0","fresh":false,"filenames":["{}"],"executable":"{}"}}"#,
                copy.display(),
                copy.display()
            ),
            format!(
                r#"{{"reason":"build-script-executed","package_id":"registry+https://x#serde@1.0.0","out_dir":"{}"}}"#,
                out.display()
            ),
        ]
        .join("\n");
        let messages = crate::cargo::messages::Messages::parse(messages.as_bytes());
        let named = super::Named::read(&messages, &[profile], None);
        let units: Vec<(&str, &str, bool, bool)> = named
            .iter()
            .map(|(hash, unit)| (hash, unit.krate(), unit.rebuilt(), unit.generated()))
            .collect();
        assert_eq!(
            units,
            [
                (hash('a').as_str(), "serde", false, false),
                (hash('b').as_str(), "rawr_generated_runner", true, true),
                (hash('d').as_str(), "serde", false, false),
            ]
        );
    }
}

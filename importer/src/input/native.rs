//! Native behavior evidence with only authenticated attempt placement removed.

use std::cmp::Ordering;
use std::path::Path;

use serde::Serialize;

use super::build;
use super::dependency::Dependency;
use super::proof::{Entry, Role};
use super::replay::canonical;
use super::stored::observe::slots::Slots;
use super::stored::{Budget, Error, Result};
use crate::cargo::unit;

const DOMAIN: &str = "rawr.input.native/v1";

/// A semantic digest derived from complete, associated normal-build evidence.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Inputs([u8; 32]);

impl Inputs {
    pub(crate) const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    #[cfg(any(test, feature = "test-fixtures"))]
    pub(in crate::input) const fn fixture(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

/// Borrowed owner geometry and bounded observations from the existing reads.
pub(in crate::input) struct Projection<'owner> {
    evidence: &'owner build::Evidence,
    harness: [&'owner Path; 3],
    harness_package: &'owner str,
    units: Box<[Unit]>,
    // Association tables are keyed by a 32-byte digest of the full key:
    // sorts and searches compare digests, and full-key equality is still
    // checked at every match and ambiguity decision.
    artifacts: Box<[([u8; 32], usize)]>,
    outputs: Box<[([u8; 32], &'owner Path, usize)]>,
    scripts: Box<[Script<'owner>]>,
}

struct Unit {
    identity: [u8; 32],
    dependency: Option<Observed>,
}

struct Observed {
    exact: [u8; 32],
    native: [u8; 32],
}

struct Script<'owner> {
    package: &'owner str,
    path: &'owner Path,
    identity: [u8; 32],
}

#[derive(Serialize)]
enum Package<'value> {
    Harness,
    Source(&'value str),
}

#[derive(Serialize)]
enum Location<'value> {
    Exact(&'value Path),
    Harness(usize),
    Generated {
        execution: [u8; 32],
        relative: &'value Path,
    },
    Artifact {
        execution: [u8; 32],
        extension: &'value str,
    },
    SavedOutput {
        execution: [u8; 32],
    },
}

impl<'owner> Projection<'owner> {
    /// Join supplied final-form files and current public artifacts to exact units.
    pub(in crate::input) fn bind(
        evidence: &'owner build::Evidence,
        harness: [&'owner Path; 3],
        graph: &unit::BuildGraph,
        command_build: &Path,
        target: &str,
        budget: &mut Budget<'_>,
    ) -> Result<Self> {
        budget.check()?;
        let mut owner = None;
        for (id, package) in &evidence.packages {
            budget.check()?;
            if package.manifest == harness[0]
                && (owner.replace(id.as_ref()).is_some()
                    || harness[0].parent() != Some(package.root.as_path()))
            {
                return Err(Error::Encoding(
                    "native harness package association ambiguous",
                ));
            }
        }
        let harness_package = owner.ok_or(Error::Encoding(
            "native harness package association missing",
        ))?;
        let mut expected = Slots::new();
        for output in graph.artifacts() {
            expected.push((output_key_digest(output, budget)?, output), budget)?;
        }
        sort(expected.as_mut_slice(), budget, |left, right, budget| {
            compare_digest(&left.0, &right.0, budget)
        })?;
        let expected = expected.as_slice();
        let mut units = Slots::new();
        for artifact in &evidence.artifacts {
            units.reserve(budget)?;
            let platform =
                build::platform::target(&artifact.dep_info, command_build, target, budget)?
                    .then_some(target);
            let digest = artifact_key_digest(artifact, platform, budget)?;
            let mut index = lower_bound(expected, &digest, |entry| &entry.0, budget)?;
            let mut matched = None;
            while let Some((candidate, output)) = expected.get(index) {
                if compare_digest(candidate, &digest, budget)? != Ordering::Equal {
                    break;
                }
                budget.check()?;
                if output.platform() == platform
                    && super::compare_artifact(output, artifact) == Ordering::Equal
                    && matched.replace(*output).is_some()
                {
                    return Err(Error::Encoding(
                        "native artifact unit association ambiguous",
                    ));
                }
                index += 1;
            }
            let output =
                matched.ok_or(Error::Encoding("native artifact unit association missing"))?;
            let package = package(output.package_id(), harness_package);
            let target = harness_path(
                output.package_id(),
                output.target(),
                harness_package,
                harness,
                budget,
            )?;
            let identity = hash(
                &(
                    "unit",
                    package,
                    target,
                    output.target_kinds(),
                    output.crate_types(),
                    output.crate_name(),
                    output.features(),
                    output.artifact_profile().as_bytes(),
                    output.platform(),
                    output.mode(),
                ),
                budget,
            )?;
            units.push(
                Unit {
                    identity,
                    dependency: None,
                },
                budget,
            )?;
        }
        let units = units.finish(budget)?;
        let mut artifacts = Slots::new();
        let mut outputs = Slots::new();
        for (index, artifact) in evidence.artifacts.iter().enumerate() {
            artifacts.push(
                (
                    dep_info_digest(&artifact.package_id, &artifact.dep_info, budget)?,
                    index,
                ),
                budget,
            )?;
            for output in &artifact.outputs {
                outputs.push(
                    (output_path_digest(output, budget)?, output.as_path(), index),
                    budget,
                )?;
            }
        }
        sort(artifacts.as_mut_slice(), budget, |left, right, budget| {
            compare_digest(&left.0, &right.0, budget)
        })?;
        for pair in artifacts.as_slice().windows(2) {
            if compare_digest(&pair[0].0, &pair[1].0, budget)? != Ordering::Equal {
                continue;
            }
            let left = &evidence.artifacts[pair[0].1];
            let right = &evidence.artifacts[pair[1].1];
            if compare_path(
                (&left.package_id, &left.dep_info),
                (&right.package_id, &right.dep_info),
                budget,
            )? == Ordering::Equal
            {
                return Err(Error::Encoding("native dep-info association ambiguous"));
            }
        }
        sort(outputs.as_mut_slice(), budget, |left, right, budget| {
            compare_digest(&left.0, &right.0, budget)
        })?;
        let mut scripts = Slots::new();
        for script in &evidence.scripts {
            scripts.reserve(budget)?;
            let (index, is_target) = script.compiler.ok_or(Error::Encoding(
                "native script compiler association missing",
            ))?;
            let artifact = evidence
                .artifacts
                .get(index)
                .filter(|artifact| artifact.package_id == script.package_id)
                .ok_or(Error::Encoding("native script compiler differs"))?;
            let unit = units
                .get(index)
                .ok_or(Error::Encoding("native script unit missing"))?;
            let identity = hash(
                &(
                    "script",
                    package(&artifact.package_id, harness_package),
                    unit.identity,
                    is_target,
                ),
                budget,
            )?;
            scripts.push(
                Script {
                    package: &script.package_id,
                    path: &script.out_dir,
                    identity,
                },
                budget,
            )?;
        }
        sort(scripts.as_mut_slice(), budget, |left, right, budget| {
            compare_path(
                (left.package, left.path),
                (right.package, right.path),
                budget,
            )
        })?;
        for pair in scripts.as_slice().windows(2) {
            budget.check()?;
            if pair[0].package == pair[1].package && pair[1].path.starts_with(pair[0].path) {
                return Err(Error::Encoding(
                    "native script directory associations overlap",
                ));
            }
        }
        Ok(Self {
            evidence,
            harness,
            harness_package,
            units,
            artifacts: artifacts.finish(budget)?,
            outputs: outputs.finish(budget)?,
            scripts: scripts.finish(budget)?,
        })
    }

    /// Consume the structured dep-info already read by the ordinary producer.
    pub(in crate::input) fn observe(
        &mut self,
        index: usize,
        dependency: &Dependency,
        budget: &mut Budget<'_>,
    ) -> Result<()> {
        let artifact = self
            .evidence
            .artifacts
            .get(index)
            .ok_or(Error::Encoding("native dependency artifact missing"))?;
        let mut sources = Slots::new();
        for source in &dependency.sources {
            sources.push(
                hash(
                    &self.path(Some(&artifact.package_id), source, budget)?,
                    budget,
                )?,
                budget,
            )?;
        }
        sort(sources.as_mut_slice(), budget, compare_digest)?;
        let sources = sources.finish(budget)?;
        let mut environment = Slots::new();
        for (key, value) in &dependency.environment {
            environment.push(hash(&(key, value), budget)?, budget)?;
        }
        sort(environment.as_mut_slice(), budget, compare_digest)?;
        let environment = environment.finish(budget)?;
        let exact = super::replay::observed::dependency_digest(dependency, budget)?;
        let native = hash(&("dependency", sources, environment), budget)?;
        let unit = self
            .units
            .get_mut(index)
            .ok_or(Error::Encoding("native dependency unit missing"))?;
        if unit
            .dependency
            .replace(Observed { exact, native })
            .is_some()
        {
            return Err(Error::Encoding("native dependency observed twice"));
        }
        Ok(())
    }

    /// Retain every exact fact, projecting only fields with a current owner join.
    pub(in crate::input) fn finish(
        self,
        entries: &[Entry],
        budget: &mut Budget<'_>,
    ) -> Result<Inputs> {
        if self.units.iter().any(|unit| unit.dependency.is_none()) {
            return Err(Error::Encoding("native dependency observations incomplete"));
        }
        let mut facts = Slots::new();
        for entry in entries {
            facts.reserve(budget)?;
            facts.push(self.entry(entry, budget)?, budget)?;
        }
        // Exact proof order includes placement. Sort projected digests instead;
        // retain multiplicity so projection cannot silently erase observations.
        sort(facts.as_mut_slice(), budget, compare_digest)?;
        let facts = facts.finish(budget)?;
        budget.storage(1, std::mem::size_of::<Option<Inputs>>())?;
        Ok(Inputs(hash(&(DOMAIN, facts), budget)?))
    }

    fn entry(&self, entry: &Entry, budget: &mut Budget<'_>) -> Result<[u8; 32]> {
        match entry {
            Entry::File {
                role,
                package_id,
                path,
                bytes,
                digest,
            } => {
                let location = if *role == Role::Harness && package_id.is_none() {
                    let slot = harness_slot(Path::new(path.as_ref()), self.harness, budget)?
                        .ok_or(Error::Encoding("native harness file differs from owner"))?;
                    Location::Harness(slot)
                } else if *role == Role::BuiltOutput && package_id.is_none() {
                    self.output(Path::new(path.as_ref()), budget)?
                } else {
                    self.path(package_id.as_deref(), Path::new(path.as_ref()), budget)?
                };
                hash(
                    &(
                        "file",
                        role,
                        self.package(package_id.as_deref()),
                        location,
                        bytes,
                        digest,
                    ),
                    budget,
                )
            }
            Entry::Directory {
                role,
                package_id,
                path,
                directories,
                files,
            } => {
                let location =
                    self.path(package_id.as_deref(), Path::new(path.as_ref()), budget)?;
                hash(
                    &(
                        "directory",
                        role,
                        self.package(package_id.as_deref()),
                        location,
                        directories,
                        files,
                    ),
                    budget,
                )
            }
            Entry::Missing {
                role,
                package_id,
                path,
            } => {
                let location =
                    self.path(package_id.as_deref(), Path::new(path.as_ref()), budget)?;
                hash(
                    &(
                        "missing",
                        role,
                        self.package(package_id.as_deref()),
                        location,
                    ),
                    budget,
                )
            }
            Entry::Environment {
                package_id,
                execution,
                key,
                value,
            } => {
                let execution = self.execution(package_id, execution, budget)?;
                hash(
                    &(
                        "environment",
                        self.package(Some(package_id)),
                        execution,
                        key,
                        value,
                    ),
                    budget,
                )
            }
            Entry::Fact {
                role: Role::DepInfo,
                package_id: Some(package_id),
                name,
                value,
            } => {
                let path = name
                    .strip_prefix(package_id.as_ref())
                    .and_then(|value| value.strip_prefix(':'))
                    .ok_or(Error::Encoding("native dep-info fact name differs"))?;
                let index = self.artifact(package_id, Path::new(path), budget)?;
                let unit = &self.units[index];
                let observed = unit
                    .dependency
                    .as_ref()
                    .ok_or(Error::Encoding("native dep-info observation absent"))?;
                budget.check()?;
                if !hex_matches(&observed.exact, value) {
                    return Err(Error::Encoding("native dep-info differs from exact proof"));
                }
                hash(&("dep-info", unit.identity, observed.native), budget)
            }
            Entry::Fact {
                role: Role::DependencySource,
                package_id: Some(package_id),
                name,
                value,
            } if package_id.as_ref() == self.harness_package => {
                let package = self
                    .evidence
                    .packages
                    .get(package_id)
                    .ok_or(Error::Encoding("native harness package missing"))?;
                budget.check()?;
                if name.strip_prefix("package:") != Some(package_id.as_ref())
                    || Path::new(value.as_ref()) != package.root
                {
                    return Err(Error::Encoding("native harness root fact differs"));
                }
                hash(&("package-root", Package::Harness), budget)
            }
            Entry::BuildMessage {
                package_id,
                execution,
                name,
                source,
                value,
            } => {
                let identity = self
                    .script(package_id, Path::new(execution.as_ref()), budget)?
                    .ok_or(Error::Encoding("native build message execution missing"))?;
                let source = match source {
                    Some(source) => {
                        budget.check()?;
                        if name.as_ref() == "saved-output"
                            && Path::new(source.as_ref()).parent()
                                == Path::new(execution.as_ref()).parent()
                            && Path::new(source.as_ref())
                                .file_name()
                                .is_some_and(|name| name == "output")
                        {
                            Some(Location::SavedOutput {
                                execution: identity,
                            })
                        } else {
                            Some(Location::Exact(Path::new(source.as_ref())))
                        }
                    }
                    None => None,
                };
                hash(
                    &(
                        "build-message",
                        self.package(Some(package_id)),
                        identity,
                        name,
                        source,
                        value,
                    ),
                    budget,
                )
            }
            Entry::Directive {
                package_id,
                execution,
                index,
                value,
            } => {
                let execution = self
                    .script(package_id, Path::new(execution.as_ref()), budget)?
                    .ok_or(Error::Encoding("native directive execution missing"))?;
                hash(
                    &(
                        "directive",
                        self.package(Some(package_id)),
                        execution,
                        index,
                        value,
                    ),
                    budget,
                )
            }
            _ => hash(&("exact", entry), budget),
        }
    }

    fn package<'value>(&self, value: Option<&'value str>) -> Option<Package<'value>> {
        value.map(|value| package(value, self.harness_package))
    }

    fn path<'value>(
        &self,
        package: Option<&str>,
        path: &'value Path,
        budget: &mut Budget<'_>,
    ) -> Result<Location<'value>> {
        if package == Some(self.harness_package) {
            for (slot, candidate) in self.harness.iter().enumerate() {
                budget.check()?;
                if path == *candidate {
                    return Ok(Location::Harness(slot));
                }
            }
        }
        if let Some(package) = package
            && let Some(script) = self.script_before(package, path, budget)?
        {
            budget.check()?;
            if let Ok(relative) = path.strip_prefix(script.path) {
                return Ok(Location::Generated {
                    execution: script.identity,
                    relative,
                });
            }
        }
        Ok(Location::Exact(path))
    }

    fn artifact(&self, package: &str, path: &Path, budget: &mut Budget<'_>) -> Result<usize> {
        let digest = dep_info_digest(package, path, budget)?;
        let mut position = lower_bound(&self.artifacts, &digest, |entry| &entry.0, budget)?;
        while let Some((candidate, index)) = self.artifacts.get(position) {
            if compare_digest(candidate, &digest, budget)? != Ordering::Equal {
                break;
            }
            let artifact = &self.evidence.artifacts[*index];
            if compare_path(
                (&artifact.package_id, &artifact.dep_info),
                (package, path),
                budget,
            )? == Ordering::Equal
            {
                return Ok(*index);
            }
            position += 1;
        }
        Err(Error::Encoding("native dep-info association missing"))
    }

    fn execution(&self, package: &str, path: &str, budget: &mut Budget<'_>) -> Result<[u8; 32]> {
        let path = Path::new(path);
        if let Some(script) = self.script(package, path, budget)? {
            return Ok(script);
        }
        Ok(self.units[self.artifact(package, path, budget)?].identity)
    }

    fn script(
        &self,
        package: &str,
        path: &Path,
        budget: &mut Budget<'_>,
    ) -> Result<Option<[u8; 32]>> {
        if let Some(script) = self.script_before(package, path, budget)? {
            budget.check()?;
            if script.path == path {
                return Ok(Some(script.identity));
            }
        }
        Ok(None)
    }

    fn script_before(
        &self,
        package: &str,
        path: &Path,
        budget: &mut Budget<'_>,
    ) -> Result<Option<&Script<'owner>>> {
        let mut first = 0;
        let mut end = self.scripts.len();
        while first < end {
            let middle = first + (end - first) / 2;
            let script = &self.scripts[middle];
            if compare_path((script.package, script.path), (package, path), budget)?
                != Ordering::Greater
            {
                first = middle + 1;
            } else {
                end = middle;
            }
        }
        let Some(index) = first.checked_sub(1) else {
            return Ok(None);
        };
        let script = &self.scripts[index];
        budget.check()?;
        Ok((script.package == package).then_some(script))
    }

    fn output<'value>(
        &self,
        path: &'value Path,
        budget: &mut Budget<'_>,
    ) -> Result<Location<'value>> {
        let digest = output_path_digest(path, budget)?;
        let mut position = lower_bound(&self.outputs, &digest, |entry| &entry.0, budget)?;
        let mut matched = None;
        while let Some((candidate, output, index)) = self.outputs.get(position) {
            if compare_digest(candidate, &digest, budget)? != Ordering::Equal {
                break;
            }
            if compare_path(("", output), ("", path), budget)? == Ordering::Equal
                && matched.replace(*index).is_some()
            {
                return Err(Error::Encoding(
                    "native selected output association ambiguous",
                ));
            }
            position += 1;
        }
        let index = matched.ok_or(Error::Encoding(
            "native selected output association missing",
        ))?;
        let extension = path
            .extension()
            .and_then(|value| value.to_str())
            .ok_or(Error::Encoding("native selected output extension missing"))?;
        let mut same_kind = 0_usize;
        for candidate in &self.evidence.artifacts[index].outputs {
            budget.check()?;
            if candidate.extension() == path.extension() {
                same_kind += 1;
            }
        }
        if same_kind != 1 {
            return Err(Error::Encoding("native selected output kind ambiguous"));
        }
        Ok(Location::Artifact {
            execution: self.units[index].identity,
            extension,
        })
    }
}

fn package<'value>(value: &'value str, harness: &str) -> Package<'value> {
    if value == harness {
        Package::Harness
    } else {
        Package::Source(value)
    }
}

fn harness_path<'value>(
    package: &str,
    path: &'value Path,
    owner: &str,
    harness: [&Path; 3],
    budget: &mut Budget<'_>,
) -> Result<Location<'value>> {
    budget.check()?;
    if package == owner
        && let Some(slot) = harness_slot(path, harness, budget)?
    {
        return Ok(Location::Harness(slot));
    }
    Ok(Location::Exact(path))
}

fn harness_slot(
    path: &Path,
    harness: [&Path; 3],
    budget: &mut Budget<'_>,
) -> Result<Option<usize>> {
    for (slot, candidate) in harness.iter().enumerate() {
        budget.check()?;
        if path == *candidate {
            return Ok(Some(slot));
        }
    }
    Ok(None)
}

/// Preserve the original refusal category when the cold owner uses this producer.
pub(in crate::input) fn capture_error(error: Error) -> crate::Error {
    match error {
        Error::Input(error) => error,
        Error::Graph(source) | Error::Bundle(source) => crate::Error::Node {
            operation: "native input projection",
            source: Box::new(source),
        },
        Error::Allocation(source) => crate::Error::Allocation {
            operation: "native input projection",
            source,
        },
        Error::Overflow
        | Error::Refused
        | Error::Observation(super::stored::observe::Error::Allocation) => {
            crate::Error::InputAccountRefused
        }
        error => super::input("native-input-projection", error),
    }
}

fn hash(value: &impl Serialize, budget: &mut Budget<'_>) -> Result<[u8; 32]> {
    canonical::digest(
        value,
        "native input canonical bytes",
        budget.limits.input_proof_bytes,
        budget,
    )
}

fn hex_matches(digest: &[u8; 32], value: &str) -> bool {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    value.len() == 64
        && digest
            .iter()
            .zip(value.as_bytes().as_chunks::<2>().0.iter())
            .all(|(byte, pair)| {
                pair[0] == HEX[usize::from(byte >> 4)] && pair[1] == HEX[usize::from(byte & 15)]
            })
}

fn compare_path(
    left: (&str, &Path),
    right: (&str, &Path),
    budget: &mut Budget<'_>,
) -> Result<Ordering> {
    budget.check()?;
    Ok(left.cmp(&right))
}

fn compare_digest(left: &[u8; 32], right: &[u8; 32], budget: &mut Budget<'_>) -> Result<Ordering> {
    budget.check()?;
    Ok(left.cmp(right))
}

// Full-key digests for the association tables. Each key is hashed once
// on the hash axis; paths hash as their encoded bytes so equality matches the
// `OsStr` comparison the final full-key check still performs.
fn output_key_digest(output: &unit::Output, budget: &mut Budget<'_>) -> Result<[u8; 32]> {
    hash(
        &(
            "artifact-key",
            output.package_id(),
            output.target().as_os_str().as_encoded_bytes(),
            output.target_kinds(),
            output.crate_types(),
            output.crate_name(),
            output.features(),
            output.artifact_profile().as_bytes(),
            output.platform(),
        ),
        budget,
    )
}

fn artifact_key_digest(
    artifact: &build::Artifact,
    platform: Option<&str>,
    budget: &mut Budget<'_>,
) -> Result<[u8; 32]> {
    let key = &artifact.key;
    hash(
        &(
            "artifact-key",
            artifact.package_id.as_ref(),
            key.target.as_os_str().as_encoded_bytes(),
            key.target_kinds.as_ref(),
            key.crate_types.as_ref(),
            key.crate_name.as_ref(),
            key.features.as_ref(),
            artifact.profile.as_bytes(),
            platform,
        ),
        budget,
    )
}

fn dep_info_digest(package: &str, path: &Path, budget: &mut Budget<'_>) -> Result<[u8; 32]> {
    hash(
        &("dep-info", package, path.as_os_str().as_encoded_bytes()),
        budget,
    )
}

fn output_path_digest(path: &Path, budget: &mut Budget<'_>) -> Result<[u8; 32]> {
    hash(&("output", path.as_os_str().as_encoded_bytes()), budget)
}

/// First position whose digest is not below `digest` in a digest-sorted table.
fn lower_bound<T>(
    values: &[T],
    digest: &[u8; 32],
    key: impl Fn(&T) -> &[u8; 32],
    budget: &mut Budget<'_>,
) -> Result<usize> {
    let mut first = 0;
    let mut end = values.len();
    while first < end {
        let middle = first + (end - first) / 2;
        if compare_digest(key(&values[middle]), digest, budget)? == Ordering::Less {
            first = middle + 1;
        } else {
            end = middle;
        }
    }
    Ok(first)
}

// Sorting owns no second index. Each comparison is admitted before reading
// its keys; after a refusal the sort performs no further evidence work.
fn sort<T>(
    values: &mut [T],
    budget: &mut Budget<'_>,
    mut compare: impl FnMut(&T, &T, &mut Budget<'_>) -> Result<Ordering>,
) -> Result<()> {
    let mut failure = None;
    values.sort_unstable_by(|left, right| {
        if failure.is_some() {
            return Ordering::Equal;
        }
        match compare(left, right, budget) {
            Ok(order) => order,
            Err(error) => {
                failure = Some(error);
                Ordering::Equal
            }
        }
    });
    match failure {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

//! Shared protected-entry semantics with separate cold and current ownership.

use std::path::Path;

mod saved;

use super::account::index::Index as _;
use super::account::{Account, Buffer, Current, Legacy};
use crate::cargo;
use crate::input::proof::{
    ArtifactPlatform, ArtifactRole, Entry, ExecutionJoin, ProtectedEntry, ProtectedExecution,
    ProtectedSource, Role,
};
use crate::input::{build, stored, Allocation};

#[cfg(test)]
pub(in crate::input) fn legacy(
    entries: &[Entry],
    evidence: &build::Evidence,
    protected: &cargo::universe::MetadataProof,
    target: &str,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> crate::Result<cargo::universe::BuildProjection> {
    legacy_accounted(
        entries,
        evidence,
        protected,
        target,
        limits,
        allocation,
        &crate::node::GraphBudget::new(),
    )
}

pub(in crate::input) fn legacy_accounted(
    entries: &[Entry],
    evidence: &build::Evidence,
    protected: &cargo::universe::MetadataProof,
    target: &str,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
    graph: &crate::node::GraphBudget,
) -> crate::Result<cargo::universe::BuildProjection> {
    project(
        entries,
        evidence,
        protected,
        target,
        limits,
        &mut Legacy(allocation, graph),
    )
}

pub(in crate::input) fn current(
    entries: &[Entry],
    evidence: &build::Evidence,
    protected: &cargo::universe::MetadataProof,
    target: &str,
    limits: &cargo::Limits,
    budget: &mut stored::Budget<'_>,
) -> stored::Result<cargo::universe::BuildProjection> {
    budget.check()?;
    let mut limits = *limits;
    limits.items = limits.items.min(budget.limits.items);
    limits.units = limits.units.min(budget.limits.units);
    limits.packages = limits.packages.min(budget.limits.packages);
    limits.input_proof_bytes = limits
        .input_proof_bytes
        .min(budget.limits.input_proof_bytes);
    project(
        entries,
        evidence,
        protected,
        target,
        &limits,
        &mut Current(budget),
    )
}

fn project<'evidence, A: Account>(
    entries: &'evidence [Entry],
    evidence: &'evidence build::Evidence,
    protected: &cargo::universe::MetadataProof,
    target: &str,
    limits: &cargo::Limits,
    allocation: &mut A,
) -> Result<cargo::universe::BuildProjection, A::Error> {
    allocation.check()?;
    struct OwnerIndex<'evidence> {
        packages: &'evidence build::packages::Table,
        roots: Vec<(&'evidence str, &'evidence Path)>,
    }

    impl<'evidence> OwnerIndex<'evidence> {
        fn new<A: Account>(
            evidence: &'evidence build::Evidence,
            limits: &cargo::Limits,
            allocation: &mut A,
        ) -> Result<Self, A::Error> {
            allocation.limited(
                "protected build package owner index",
                limits.packages,
                evidence.packages.len(),
            )?;
            let mut roots = allocation.roots(evidence.packages.len())?;
            for (package_id, package) in &evidence.packages {
                roots.push((package_id.as_ref(), package.root.as_path()));
            }
            // By root, so each ancestor of a path is one binary search.
            roots.sort_unstable_by(|left, right| {
                left.1.cmp(right.1).then_with(|| left.0.cmp(right.0))
            });
            Ok(Self {
                packages: &evidence.packages,
                roots,
            })
        }

        fn owner<A: Account>(
            &self,
            path: &Path,
            allocation: &mut A,
        ) -> Result<Option<(&'evidence str, &'evidence build::Package)>, A::Error> {
            // The deepest package root containing the path owns it.
            for ancestor in path.ancestors() {
                let start = self.roots.partition_point(|(_, root)| *root < ancestor);
                let mut matches = self.roots[start..]
                    .iter()
                    .take_while(|(_, root)| *root == ancestor);
                let Some(&(owner, _)) = matches.next() else {
                    continue;
                };
                if matches.any(|(candidate, _)| *candidate != owner) {
                    return Err(allocation
                        .policy("protected-build-package-owner-ambiguous", path.display()));
                }
                return Ok(Some((owner, self.package(owner, allocation)?)));
            }
            Ok(None)
        }

        fn package<A: Account>(
            &self,
            package_id: &str,
            allocation: &mut A,
        ) -> Result<&'evidence build::Package, A::Error> {
            self.packages
                .get(package_id)
                .ok_or_else(|| allocation.policy("protected-build-package-unmapped", package_id))
        }
    }

    fn protected_package_path<A: Account>(
        package_id: &str,
        path: &Path,
        allow_external: bool,
        allow_cross_package: bool,
        owners: &OwnerIndex<'_>,
        protected: &cargo::universe::MetadataProof,
        allocation: &mut A,
    ) -> Result<Option<ProtectedSource>, A::Error> {
        owners.package(package_id, allocation)?;
        let owner = owners.owner(path, allocation)?;
        let Some(consumer) = allocation.token(protected, package_id)? else {
            // A package the facade does not reach is outside the
            // protected projection. Rust lets its `#[path]` modules live
            // outside its package root (Rawr's generated app crate reads the
            // project's `src/`); its dep-info still tracks them.
            return Ok(None);
        };
        let Some((owner_id, owner_package)) = owner else {
            let code = if allow_external {
                "protected-build-external-input-unmapped"
            } else {
                "protected-build-package-path-unmapped"
            };
            return Err(allocation.policy(code, path.display()));
        };
        if !allow_cross_package && owner_id != package_id {
            return Err(allocation.policy("protected-build-package-owner-conflict", path.display()));
        }
        let owner = allocation.token(protected, owner_id)?.ok_or_else(|| {
            allocation.policy("protected-build-source-owner-unprotected", path.display())
        })?;
        let relative = path.strip_prefix(&owner_package.root).map_err(|_| {
            allocation.policy("protected-build-package-path-unmapped", path.display())
        })?;
        let normalized = allocation.path(&owner, relative, path)?;
        Ok(Some(ProtectedSource {
            consumer: allocation.boxed(consumer)?,
            owner: allocation.boxed(owner)?,
            path: allocation.boxed(normalized)?,
        }))
    }

    fn artifact_platform(path: &Path, target: &str) -> ArtifactPlatform {
        if path
            .components()
            .any(|component| component.as_os_str() == target)
        {
            ArtifactPlatform::Target
        } else {
            ArtifactPlatform::Host
        }
    }

    fn artifact_role(artifact: &build::Artifact) -> ArtifactRole {
        if artifact
            .key
            .target_kinds
            .iter()
            .any(|kind| kind.as_ref() == "custom-build")
        {
            ArtifactRole::BuildScript
        } else if artifact
            .key
            .crate_types
            .iter()
            .any(|kind| kind.as_ref() == "proc-macro")
        {
            ArtifactRole::ProcMacro
        } else {
            ArtifactRole::Normal
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn protected_execution<'evidence, A: Account>(
        package_id: &'evidence str,
        raw: &'evidence str,
        scripts: &A::Index<(&'evidence str, &'evidence Path), &'evidence build::Script>,
        artifacts: &A::Index<(&'evidence str, &'evidence Path), &'evidence build::Artifact>,
        build_scripts: &'evidence [build::Artifact],
        owners: &OwnerIndex<'_>,
        protected: &cargo::universe::MetadataProof,
        target: &str,
        allocation: &mut A,
    ) -> Result<ProtectedExecution, A::Error> {
        let path = Path::new(raw);
        if let Some(script) = scripts.get(&(package_id, path)) {
            let script = match script {
                ExecutionJoin::One(script) => script,
                ExecutionJoin::Ambiguous => {
                    return Err(
                        allocation.policy("protected-build-script-execution-ambiguous", raw)
                    );
                }
            };
            let (index, is_target) = script.compiler.ok_or_else(|| {
                allocation.policy("protected-build-script-artifact-unmapped", package_id)
            })?;
            let platform = if is_target {
                ArtifactPlatform::Target
            } else {
                ArtifactPlatform::Host
            };
            let artifact = build_scripts
                .get(index)
                .filter(|artifact| {
                    artifact.package_id.as_ref() == package_id
                        && artifact_role(artifact) == ArtifactRole::BuildScript
                })
                .ok_or_else(|| {
                    allocation.policy("protected-build-script-artifact-unmapped", package_id)
                })?;
            let target = protected_package_path(
                package_id,
                &artifact.key.target,
                false,
                false,
                owners,
                protected,
                allocation,
            )?
            .ok_or_else(|| {
                allocation.policy("protected-build-script-package-unmapped", package_id)
            })?
            .path;
            return Ok(ProtectedExecution::BuildScript {
                target,
                target_kinds: allocation.strings(&artifact.key.target_kinds)?,
                crate_types: allocation.strings(&artifact.key.crate_types)?,
                crate_name: allocation.text(&artifact.key.crate_name)?,
                features: allocation.strings(&artifact.key.features)?,
                profile: allocation.bytes(artifact.profile.as_bytes())?,
                platform,
            });
        }

        let artifact = match artifacts.get(&(package_id, path)) {
            Some(ExecutionJoin::One(artifact)) => artifact,
            Some(ExecutionJoin::Ambiguous) => {
                return Err(allocation.policy("protected-environment-artifact-ambiguous", raw));
            }
            None => {
                return Err(allocation.policy("protected-environment-artifact-unmapped", raw));
            }
        };
        let normalized_target = protected_package_path(
            package_id,
            &artifact.key.target,
            false,
            false,
            owners,
            protected,
            allocation,
        )?
        .ok_or_else(|| allocation.policy("protected-artifact-target-unmapped", package_id))?
        .path;
        Ok(ProtectedExecution::Artifact {
            target: normalized_target,
            target_kinds: allocation.strings(&artifact.key.target_kinds)?,
            crate_types: allocation.strings(&artifact.key.crate_types)?,
            crate_name: allocation.text(&artifact.key.crate_name)?,
            features: allocation.strings(&artifact.key.features)?,
            profile: allocation.bytes(artifact.profile.as_bytes())?,
            platform: artifact_platform(&artifact.dep_info, target),
            role: artifact_role(artifact),
        })
    }

    let owners = OwnerIndex::new(evidence, limits, allocation)?;
    let mut scripts = A::Index::new();
    for script in &evidence.scripts {
        allocation.limited(
            "protected build-script execution joins",
            limits.units,
            scripts.len().saturating_add(1),
        )?;
        allocation.join()?;
        let key = (script.package_id.as_ref(), script.out_dir.as_path());
        scripts.insert(key, script, allocation)?;
    }
    let mut artifacts = A::Index::new();
    let build_scripts = evidence.artifacts.as_ref();
    for artifact in &evidence.artifacts {
        allocation.limited(
            "protected build artifact execution joins",
            limits.units,
            artifacts.len().saturating_add(1),
        )?;
        allocation.join()?;
        let key = (artifact.package_id.as_ref(), artifact.dep_info.as_path());
        artifacts.insert(key, artifact, allocation)?;
    }

    allocation.comparison(entries.len())?;
    let count = entries
        .iter()
        .filter(|entry| matches!(entry, Entry::Directive { .. }))
        .count();
    let mut directives = allocation.roots(count)?;
    directives.extend(
        entries
            .iter()
            .filter(|entry| matches!(entry, Entry::Directive { .. })),
    );
    let inputs = entries;
    let mut entries = Buffer::new();
    for entry in inputs {
        allocation.check()?;
        let projected = match entry {
            Entry::File {
                role,
                package_id,
                path,
                bytes,
                digest,
            } if matches!(
                role,
                Role::PackageManifest
                    | Role::DependencySource
                    | Role::BuildScript
                    | Role::DeclaredInput
                    | Role::DefaultPackageSource
            ) =>
            {
                let package_id = package_id.as_deref().ok_or_else(|| {
                    allocation.policy("protected-build-package-owner-missing", path.as_ref())
                })?;
                let path = Path::new(path.as_ref());
                let package_evidence = owners.package(package_id, allocation)?;
                if *role == Role::DependencySource {
                    let mut generated = None;
                    for script in &evidence.scripts {
                        // Admit package comparison and component-wise prefix work
                        // before inspecting either value; repeated files spend
                        // the same original cumulative account.
                        allocation.comparison(
                            package_id
                                .len()
                                .min(script.package_id.len())
                                .saturating_add(1),
                        )?;
                        if script.package_id.as_ref() != package_id {
                            continue;
                        }
                        allocation.comparison(
                            path.as_os_str()
                                .len()
                                .saturating_add(script.out_dir.as_os_str().len())
                                .saturating_mul(2)
                                .saturating_add(1),
                        )?;
                        if let Ok(relative) = path.strip_prefix(&script.out_dir)
                            && generated.replace((script, relative)).is_some()
                        {
                            return Err(allocation.policy(
                                "protected-build-generated-owner-ambiguous",
                                path.display(),
                            ));
                        }
                    }
                    if let Some((script, relative)) = generated {
                        let Some(package) = allocation.token(protected, package_id)? else {
                            continue;
                        };
                        if relative.as_os_str().is_empty()
                            || relative
                                .components()
                                .any(|part| !matches!(part, std::path::Component::Normal(_)))
                        {
                            return Err(allocation
                                .policy("protected-build-generated-path-shape", path.display()));
                        }
                        let relative = relative.to_str().ok_or_else(|| {
                            allocation
                                .policy("protected-build-generated-path-encoding", path.display())
                        })?;
                        let execution = script.out_dir.to_str().ok_or_else(|| {
                            allocation
                                .policy("protected-build-generated-path-encoding", path.display())
                        })?;
                        let projected = ProtectedEntry::GeneratedFile {
                            package: allocation.boxed(package)?,
                            execution: protected_execution(
                                package_id,
                                execution,
                                &scripts,
                                &artifacts,
                                build_scripts,
                                &owners,
                                protected,
                                target,
                                allocation,
                            )?,
                            path: allocation.text(relative)?,
                            bytes: *bytes,
                            digest: *digest,
                        };
                        allocation.limited(
                            "protected source/build projection entries",
                            limits.items,
                            entries.values.len().saturating_add(1),
                        )?;
                        allocation.push(&mut entries, projected)?;
                        continue;
                    }
                }
                match role {
                    Role::PackageManifest if package_evidence.manifest != path => {
                        return Err(allocation
                            .policy("protected-build-package-manifest-conflict", path.display()));
                    }
                    Role::BuildScript if package_evidence.build_script.as_deref() != Some(path) => {
                        return Err(allocation
                            .policy("protected-build-script-source-conflict", path.display()));
                    }
                    _ => {}
                }
                let allow_external = *role == Role::DeclaredInput;
                let allow_cross_package =
                    matches!(role, Role::DependencySource | Role::DeclaredInput);
                let Some(source) = protected_package_path(
                    package_id,
                    path,
                    allow_external,
                    allow_cross_package,
                    &owners,
                    protected,
                    allocation,
                )?
                else {
                    continue;
                };
                Some(ProtectedEntry::File {
                    role: *role,
                    source,
                    bytes: *bytes,
                    digest: *digest,
                })
            }
            Entry::Directory {
                role: Role::OutDirectory,
                package_id,
                path,
                directories,
                files,
            } => {
                let package_id = package_id.as_deref().ok_or_else(|| {
                    allocation.policy("protected-build-package-owner-missing", path.as_ref())
                })?;
                owners.package(package_id, allocation)?;
                let Some(package) = allocation.token(protected, package_id)? else {
                    continue;
                };
                Some(ProtectedEntry::OutputDirectory {
                    package: allocation.boxed(package)?,
                    execution: protected_execution(
                        package_id,
                        path,
                        &scripts,
                        &artifacts,
                        build_scripts,
                        &owners,
                        protected,
                        target,
                        allocation,
                    )?,
                    directories: allocation.directories(directories)?,
                    files: allocation.files(files)?,
                })
            }
            Entry::Directory {
                role: Role::DefaultPackageSource,
                package_id,
                path,
                directories,
                files,
            } => {
                let package_id = package_id.as_deref().ok_or_else(|| {
                    allocation.policy("protected-build-package-owner-missing", path.as_ref())
                })?;
                let package_evidence = owners.package(package_id, allocation)?;
                if package_evidence.root != Path::new(path.as_ref()) {
                    return Err(
                        allocation.policy("protected-build-package-root-conflict", path.as_ref())
                    );
                }
                let Some(source) = protected_package_path(
                    package_id,
                    Path::new(path.as_ref()),
                    false,
                    false,
                    &owners,
                    protected,
                    allocation,
                )?
                else {
                    continue;
                };
                Some(ProtectedEntry::SourceDirectory {
                    package: source.consumer,
                    directories: allocation.directories(directories)?,
                    files: allocation.files(files)?,
                })
            }
            Entry::Registry {
                role,
                package_id,
                source,
                checksum,
                marker_schema,
                marker_digest,
                root,
            } => {
                let package = owners.package(package_id, allocation)?;
                if package.root != Path::new(root.as_ref()) {
                    return Err(
                        allocation.policy("protected-build-package-root-conflict", root.as_ref())
                    );
                }
                protected_package_path(
                    package_id,
                    Path::new(root.as_ref()),
                    false,
                    false,
                    &owners,
                    protected,
                    allocation,
                )?
                .map(|path| {
                    Ok::<_, A::Error>(ProtectedEntry::Registry {
                        role: *role,
                        package: path.consumer,
                        source: allocation.text(source)?,
                        checksum: *checksum,
                        marker_schema: *marker_schema,
                        marker_digest: *marker_digest,
                    })
                })
                .transpose()?
            }
            Entry::Git {
                role,
                package_id,
                source,
                oid,
                files,
                root,
            } => {
                let package = owners.package(package_id, allocation)?;
                if package.root != Path::new(root.as_ref()) {
                    return Err(
                        allocation.policy("protected-build-package-root-conflict", root.as_ref())
                    );
                }
                protected_package_path(
                    package_id,
                    Path::new(root.as_ref()),
                    false,
                    false,
                    &owners,
                    protected,
                    allocation,
                )?
                .map(|path| {
                    Ok::<_, A::Error>(ProtectedEntry::Git {
                        role: *role,
                        package: path.consumer,
                        source: allocation.text(source)?,
                        oid: allocation.text(oid)?,
                        files: allocation.files(files)?,
                    })
                })
                .transpose()?
            }
            Entry::Missing {
                role,
                package_id,
                path,
            } if matches!(role, Role::DeclaredInput | Role::DependencySource) => {
                let package_id = package_id.as_deref().ok_or_else(|| {
                    allocation.policy("protected-build-package-owner-missing", path.as_ref())
                })?;
                protected_package_path(
                    package_id,
                    Path::new(path.as_ref()),
                    *role == Role::DeclaredInput,
                    true,
                    &owners,
                    protected,
                    allocation,
                )?
                .map(|source| ProtectedEntry::Missing {
                    role: *role,
                    source,
                })
            }
            Entry::Environment {
                package_id,
                execution,
                key,
                value,
            } => {
                owners.package(package_id, allocation)?;
                let Some(package) = allocation.token(protected, package_id)? else {
                    continue;
                };
                Some(ProtectedEntry::Environment {
                    package: allocation.boxed(package)?,
                    execution: protected_execution(
                        package_id,
                        execution,
                        &scripts,
                        &artifacts,
                        build_scripts,
                        &owners,
                        protected,
                        target,
                        allocation,
                    )?,
                    key: allocation.text(key)?,
                    value: value
                        .as_deref()
                        .map(|value| {
                            let value = allocation.normalized(value, evidence, protected)?;
                            allocation.boxed(value)
                        })
                        .transpose()?,
                })
            }
            Entry::Fact {
                role: Role::DependencySource,
                package_id,
                name,
                value,
            } => {
                let package_id = package_id.as_deref().ok_or_else(|| {
                    allocation.policy("protected-build-package-owner-missing", name.as_ref())
                })?;
                let package_evidence = owners.package(package_id, allocation)?;
                if name.as_ref() != allocation.format(format_args!("package:{package_id}"))?
                    || Path::new(value.as_ref()) != package_evidence.root
                {
                    return Err(
                        allocation.policy("protected-build-package-root-fact-conflict", package_id)
                    );
                }
                let Some(path) = protected_package_path(
                    package_id,
                    &package_evidence.root,
                    false,
                    false,
                    &owners,
                    protected,
                    allocation,
                )?
                else {
                    continue;
                };
                let package = path.consumer;
                Some(ProtectedEntry::Fact {
                    role: Role::DependencySource,
                    package: allocation.text(&package)?,
                    name: allocation.text("package-root")?,
                    value: {
                        let value = allocation.format(format_args!("$PACKAGE/{package}"))?;
                        allocation.boxed(value)?
                    },
                })
            }
            Entry::BuildMessage {
                package_id,
                execution,
                name,
                value,
                ..
            } => {
                owners.package(package_id, allocation)?;
                let Some(package) = allocation.token(protected, package_id)? else {
                    continue;
                };
                Some(ProtectedEntry::BuildMessage {
                    package: allocation.boxed(package)?,
                    execution: protected_execution(
                        package_id,
                        execution,
                        &scripts,
                        &artifacts,
                        build_scripts,
                        &owners,
                        protected,
                        target,
                        allocation,
                    )?,
                    name: allocation.text(name)?,
                    value: {
                        if name.as_ref() == "saved-output" {
                            saved::project(
                                &directives,
                                package_id,
                                execution,
                                value,
                                evidence,
                                protected,
                                limits,
                                allocation,
                            )?
                        } else {
                            let value = allocation.normalized(value, evidence, protected)?;
                            allocation.boxed(value)?
                        }
                    },
                })
            }
            Entry::Directive {
                package_id,
                execution,
                index,
                value,
            } => {
                owners.package(package_id, allocation)?;
                let Some(package) = allocation.token(protected, package_id)? else {
                    continue;
                };
                Some(ProtectedEntry::Directive {
                    package: allocation.boxed(package)?,
                    execution: protected_execution(
                        package_id,
                        execution,
                        &scripts,
                        &artifacts,
                        build_scripts,
                        &owners,
                        protected,
                        target,
                        allocation,
                    )?,
                    index: *index,
                    value: {
                        let value = allocation.normalized(value, evidence, protected)?;
                        allocation.boxed(value)?
                    },
                })
            }
            Entry::Workspace { .. }
            | Entry::File { .. }
            | Entry::Directory { .. }
            | Entry::Missing { .. }
            | Entry::EnvironmentProjection { .. }
            | Entry::Fact { .. } => None,
        };
        if let Some(projected) = projected {
            allocation.limited(
                "protected source/build projection entries",
                limits.items,
                entries.values.len().saturating_add(1),
            )?;
            allocation.push(&mut entries, projected)?;
        }
    }
    entries.values.sort_unstable();
    entries.values.dedup();
    allocation.canonical(&entries.values, limits)
}

#[cfg(test)]
mod tests;

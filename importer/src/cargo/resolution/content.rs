//! Consumer-derived content obligations for one fixed resolution allocation.
//!
//! Complete checkpoints retain every physical member. Intermediate consumers
//! use these obligations for fresh bytes, relevant ancestors and exact namespaces;
//! selection alone grants no currency. Search policy remains the pinned
//! metadata-only probe's conservative superset.

use std::path::{Component, Path, PathBuf};

use crate::cargo::toolchain;
use crate::extract::plan::Capture;
use crate::input::{build, proof::Entry, stored};
use crate::node::{Error, GraphBudget};
use crate::store::bundle::{self, buffer::Buffer};

const COMPILER: &str = "375b1431b7d89d1c2e2bc168c011848ae12b7d14";

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum Kind {
    File,
    Search,
    Tree,
}

struct Obligation {
    path: PathBuf,
    kind: Kind,
}

/// Historical selection derived only from the completed owner's observations.
/// Its paths remain bound to the exact fixed allocation, not a relocatable key.
pub(crate) struct Scope {
    root: PathBuf,
    obligations: Vec<Obligation>,
    graph: GraphBudget,
    omitted: Vec<PathBuf>,
}

struct Builder<'a> {
    root: &'a Path,
    obligations: Buffer<Obligation>,
    budget: &'a GraphBudget,
    omitted: Vec<PathBuf>,
}

impl Scope {
    #[allow(
        clippy::too_many_arguments,
        reason = "selection joins one completed owner's exact observations"
    )]
    pub(crate) fn from_observations(
        root: &Path,
        input: &stored::Stored,
        build: &build::Retained,
        capture: &Capture,
        toolchain: &toolchain::Proof,
        target: &str,
        budget: &GraphBudget,
    ) -> Result<Self, Error> {
        Self::derive(
            Builder::bind(root, budget)?,
            input,
            build,
            capture,
            toolchain,
            target,
        )
    }

    fn derive(
        mut builder: Builder<'_>,
        input: &stored::Stored,
        build: &build::Retained,
        capture: &Capture,
        toolchain: &toolchain::Proof,
        target: &str,
    ) -> Result<Self, Error> {
        if toolchain.rustc_commit() != COMPILER
            || toolchain.host() != toolchain::HOST
            || target != toolchain::HOST
            || toolchain.target() != target
        {
            return Err(Error::Codec("resolution content consumer pin differs"));
        }
        // All Rawr-owned controls, protocol sidecars, record bytes and generated
        // sources remain content, including files not individually named below.
        for name in ["declared", "generated"] {
            let path = bundle::path(builder.root, Path::new(name), builder.budget)?;
            builder.add(&path, Kind::Tree)?;
        }
        builder.inputs(input.entries(), input.recipes())?;
        for path in build.artifact_outputs() {
            builder.build_output(path)?;
            builder.add(
                path.parent()
                    .ok_or(Error::Codec("Build output parent absent"))?,
                Kind::Search,
            )?;
        }
        for path in build.dep_info_paths() {
            builder.add(path, Kind::File)?;
        }
        for path in build.out_directories() {
            builder.script(path)?;
        }
        for (path, _) in capture.input_artifacts() {
            builder.add(path, Kind::File)?;
        }
        builder.finish()
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) fn shares_with(&self, budget: &GraphBudget) -> bool {
        self.graph.shares_with(budget)
    }

    /// Trees retain every child; search directories retain their selection namespace.
    pub(crate) fn requires_membership(&self, path: &Path) -> bool {
        !path.starts_with(&self.root)
            || self.obligations.iter().any(|entry| match entry.kind {
                Kind::Tree => path.starts_with(&entry.path),
                Kind::Search => path == entry.path,
                Kind::File => false,
            })
    }

    /// Keep the original ancestor chain of every independently required input.
    pub(crate) fn requires_directory(&self, path: &Path) -> bool {
        self.requires_membership(path)
            || self
                .obligations
                .iter()
                .any(|entry| entry.path.starts_with(path))
    }

    /// A tree/input role always wins over a search-only non-consumer.
    pub(crate) fn requires_content(&self, path: &Path) -> bool {
        if !path.starts_with(&self.root) {
            return true;
        }
        for ancestor in path.ancestors() {
            if !ancestor.starts_with(&self.root) {
                break;
            }
            if let Ok(index) = self
                .obligations
                .binary_search_by(|entry| entry.path.as_path().cmp(ancestor))
            {
                match self.obligations[index].kind {
                    Kind::Tree => return true,
                    Kind::File if ancestor == path => return true,
                    Kind::Search
                        if path.parent() == Some(ancestor)
                            && metadata_candidate(path)
                            && self
                                .omitted
                                .binary_search_by(|value| value.as_os_str().cmp(path.as_os_str()))
                                .is_err() =>
                    {
                        return true;
                    }
                    _ => {}
                }
            }
        }
        false
    }

    #[cfg(test)]
    pub(crate) fn complete(root: &Path, budget: &GraphBudget) -> Result<Self, Error> {
        let mut builder = Builder::bind(root, budget)?;
        builder.add(root, Kind::Tree)?;
        builder.finish()
    }
}

impl<'a> Builder<'a> {
    fn bind(root: &'a Path, budget: &'a GraphBudget) -> Result<Self, Error> {
        if !root.is_absolute()
            || root
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            return Err(Error::OutsideRoot(root.to_path_buf()));
        }
        Ok(Self {
            root,
            obligations: Buffer::new(),
            budget,
            omitted: Vec::new(),
        })
    }

    #[cfg(test)]
    fn namespace(
        &mut self,
        namespace: Option<&crate::extract::plan::namespace::Retained>,
    ) -> Result<(), Error> {
        let namespace = namespace.ok_or(Error::Codec("query scope namespace absent"))?;
        let mut omitted = Buffer::new();
        for member in &namespace.members {
            if member.candidate && !member.selected && member.path.starts_with(self.root) {
                if !member.path.is_absolute()
                    || member
                        .path
                        .components()
                        .any(|part| matches!(part, Component::ParentDir))
                {
                    return Err(Error::Codec("query scope namespace path invalid"));
                }
                if omitted.len() >= bundle::MAX_MEMBERS as usize {
                    return Err(Error::Codec("query scope namespace limit exceeded"));
                }
                omitted.push(copy_path(&member.path, self.budget)?, self.budget)?;
            }
        }
        let mut omitted = omitted.into_vec();
        omitted.sort_unstable_by(|left, right| left.as_os_str().cmp(right.as_os_str()));
        omitted.dedup_by(|left, right| left.as_os_str() == right.as_os_str());
        self.omitted = omitted;
        Ok(())
    }

    fn build_output(&mut self, path: &Path) -> Result<(), Error> {
        if self
            .omitted
            .binary_search_by(|value| value.as_os_str().cmp(path.as_os_str()))
            .is_err()
        {
            self.add(path, Kind::File)?;
        }
        Ok(())
    }

    fn inputs(&mut self, entries: &[Entry], recipes: &[stored::Recipe]) -> Result<(), Error> {
        for entry in entries {
            match entry {
                Entry::Workspace { paths, .. } => {
                    for path in paths {
                        self.add(Path::new(path.as_ref()), Kind::File)?;
                    }
                }
                Entry::File { path, .. } | Entry::Missing { path, .. } => {
                    self.add(Path::new(path.as_ref()), Kind::File)?
                }
                Entry::Directory { path, .. } => self.add(Path::new(path.as_ref()), Kind::Tree)?,
                Entry::Registry { root, .. } | Entry::Git { root, .. } => {
                    self.add(Path::new(root.as_ref()), Kind::Tree)?
                }
                Entry::BuildMessage {
                    source: Some(path), ..
                } => self.add(Path::new(path.as_ref()), Kind::File)?,
                Entry::Environment { .. }
                | Entry::EnvironmentProjection { .. }
                | Entry::Fact { .. }
                | Entry::BuildMessage { source: None, .. }
                | Entry::Directive { .. } => {}
            }
        }
        for recipe in recipes {
            match recipe {
                stored::Recipe::Workspace { .. } => {}
                stored::Recipe::File { path, .. } | stored::Recipe::Path { path, .. } => {
                    self.add(path, Kind::File)?
                }
                stored::Recipe::Directory { path, .. } => self.add(path, Kind::Tree)?,
                stored::Recipe::DepInfo {
                    path,
                    out_directories,
                    ..
                } => {
                    self.add(path, Kind::File)?;
                    for path in out_directories {
                        self.add(path, Kind::Tree)?;
                    }
                }
                stored::Recipe::SavedOutput { out_dir, .. } => self.script(out_dir)?,
                // Complete package-source entries above retain any package root
                // inside this allocation. External sources stay in input replay.
                stored::Recipe::Package(_)
                | stored::Recipe::Environment { .. }
                | stored::Recipe::EnvironmentProjection { .. }
                | stored::Recipe::Fact { .. }
                | stored::Recipe::BuildMessage { .. }
                | stored::Recipe::Directive { .. } => {}
            }
        }
        Ok(())
    }

    fn script(&mut self, out_dir: &Path) -> Result<(), Error> {
        // Keep the complete output tree and its saved-output/control siblings.
        self.add(out_dir, Kind::Tree)?;
        self.add(
            out_dir
                .parent()
                .ok_or(Error::Codec("script output parent absent"))?,
            Kind::Tree,
        )
    }

    fn add(&mut self, path: &Path, kind: Kind) -> Result<(), Error> {
        if !path.is_absolute()
            || path
                .components()
                .any(|part| matches!(part, Component::ParentDir))
        {
            return Err(Error::OutsideRoot(path.to_path_buf()));
        }
        let path = if path.starts_with(self.root) {
            path
        } else if kind == Kind::Tree && self.root.starts_with(path) {
            // A declared tree outside the allocation can still contain it.
            self.root
        } else {
            return Ok(());
        };
        if self.obligations.len() >= bundle::MAX_MEMBERS as usize {
            return Err(Error::Limit {
                resource: "resolution content obligations",
                limit: bundle::MAX_MEMBERS as usize,
                actual: self.obligations.len().saturating_add(1),
            });
        }
        let path = copy_path(path, self.budget)?;
        self.obligations
            .push(Obligation { path, kind }, self.budget)
    }

    fn finish(self) -> Result<Scope, Error> {
        let root = copy_path(self.root, self.budget)?;
        let mut obligations = self.obligations.into_vec();
        obligations.sort_unstable_by(|left, right| left.path.cmp(&right.path));
        obligations.dedup_by(|later, earlier| {
            if later.path != earlier.path {
                return false;
            }
            earlier.kind = earlier.kind.max(later.kind);
            true
        });
        Ok(Scope {
            root,
            obligations,
            graph: self.budget.clone(),
            omitted: self.omitted,
        })
    }
}

fn copy_path(path: &Path, budget: &GraphBudget) -> Result<PathBuf, Error> {
    let length = path.as_os_str().as_encoded_bytes().len();
    budget.decode(length)?;
    let mut owned = PathBuf::new();
    owned
        .try_reserve_exact(length)
        .map_err(|_| Error::Allocation)?;
    owned.push(path);
    Ok(owned)
}

fn metadata_candidate(path: &Path) -> bool {
    // rustc375b1431 compiler/rustc_metadata/src/locator.rs386–478:
    // rmeta/rlib/interface and target DLL candidates. Keep all prefixes and
    // filename groups, not only the historically selected/loaded artifacts.
    // A proc macro is a `.dylib` on macOS and a `.so` on Linux. The host
    // and target are checked before constructing this scope. Search
    // membership (including static-library diagnostic names) remains complete
    // in the physical inventory. Other roles can require any file's bytes.
    path.file_name()
        .and_then(|name| name.to_str())
        .is_none_or(|name| {
            [".rmeta", ".rlib", ".rs", ".dylib", ".so"]
                .iter()
                .any(|suffix| name.ends_with(suffix))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completed_query_scope_preserves_physical_inventory_and_independent_content() {
        use crate::extract::plan::namespace::Snapshot;
        use crate::extract::protocol::attempt::{self, Completion, Invocation};
        use crate::runner::artifact::retained::{Mode, Stored};
        use std::fs;
        use std::sync::atomic::{AtomicUsize, Ordering};

        static NEXT: AtomicUsize = AtomicUsize::new(0);
        struct Root(PathBuf);
        impl Drop for Root {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.0);
            }
        }
        let temporary = Root(std::env::temp_dir().join(format!(
            "rawr-query-inventory-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        )));
        fs::create_dir(&temporary.0).unwrap();
        let reference = bundle::BundleRef::new(
            crate::node::ExecutionKey::from_bytes([1; 32]),
            bundle::BundleId::from_bytes([2; 32]),
        );
        let root = fs::canonicalize(&temporary.0)
            .unwrap()
            .join(reference.relative());
        for name in ["build", "target", "declared", "generated"] {
            fs::create_dir_all(root.join(name)).unwrap();
        }
        let archive = root.join("build/libunused.rlib");
        let independent = root.join("build/libinput.rlib");
        let queried = root.join("build/libquery.rmeta");
        let external_root = fs::canonicalize(&temporary.0).unwrap().join("external");
        fs::create_dir(&external_root).unwrap();
        let external = external_root.join("libexternal.rmeta");
        fs::write(&external, b"before").unwrap();
        for path in [&archive, &independent, &queried] {
            fs::write(path, b"before").unwrap();
        }
        let graph = GraphBudget::new();
        let mut allocation = crate::extract::Allocation::with_limits(usize::MAX, usize::MAX);
        let observed = Snapshot::capture(
            &[root.join("build"), external_root],
            &[],
            &mut allocation,
            &graph,
            1024,
        )
        .unwrap();
        let id = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let mut bytes = Vec::new();
        attempt::encode(
            &mut bytes,
            id,
            false,
            &[Invocation {
                nonce: id,
                correlation: id,
            }],
            std::iter::empty(),
            [queried.to_str().unwrap(), external.to_str().unwrap()].into_iter(),
        )
        .unwrap();
        let completion = Completion::decode(&bytes).unwrap();
        let mut namespace = observed
            .complete(&completion, &mut allocation, &graph, 1024)
            .unwrap();
        namespace
            .require_selection(&completion, &mut allocation, &graph)
            .unwrap();
        let mut builder = Builder::bind(&root, &graph).unwrap();
        builder.namespace(Some(&namespace)).unwrap();
        builder.add(&root.join("build"), Kind::Search).unwrap();
        for path in [&archive, &independent, &queried] {
            builder.build_output(path).unwrap();
        }
        builder.add(&independent, Kind::File).unwrap();
        let scope = builder.finish().unwrap();
        let stored =
            Stored::capture_resolution(&root, reference, Mode::Pending, &graph, &scope).unwrap();
        let before = graph.census_usage().unwrap().2;
        stored.replay(&root, Mode::Pending, &graph).unwrap();
        namespace.revalidate(&mut allocation, &graph, 1024).unwrap();
        let separate = graph.census_usage().unwrap().2 - before;
        let before = graph.census_usage().unwrap().2;
        assert_eq!(
            namespace
                .revalidate_inventory(&stored, &root, Mode::Pending, &mut allocation, &graph, 1024,)
                .unwrap(),
            6
        );
        let joined = graph.census_usage().unwrap().2 - before;
        // The six-byte queried member and its EOF probe are read once
        // and shared, whether the two consumers run separately or joined.
        assert_eq!(separate, joined);
        let selected = namespace
            .members
            .iter()
            .position(|member| member.path == queried)
            .unwrap();
        let digest = namespace.members[selected].digest.replace([0; 32]);
        assert!(namespace
            .revalidate_inventory(&stored, &root, Mode::Pending, &mut allocation, &graph, 1024)
            .is_err());
        namespace.members[selected].digest = digest;
        namespace.members.swap(0, 1);
        assert!(namespace
            .revalidate_inventory(&stored, &root, Mode::Pending, &mut allocation, &graph, 1024)
            .is_err());
        namespace.members.swap(0, 1);
        fs::write(&external, b"after!").unwrap();
        assert!(namespace
            .revalidate_inventory(&stored, &root, Mode::Pending, &mut allocation, &graph, 1024)
            .is_err());
        fs::write(&external, b"before").unwrap();
        // A graph account spent to its target is not a gate. Revalidation
        // on such an account still completes and records the crossing axis.
        for resource in 0..3 {
            let exhausted = GraphBudget::new();
            let (name, target) = match resource {
                0 => {
                    exhausted
                        .hash(crate::node::MAX_HASHED_BYTES_PER_GRAPH as usize)
                        .unwrap();
                    (
                        crate::node::HASHED_RESOURCE,
                        crate::node::MAX_HASHED_BYTES_PER_GRAPH,
                    )
                }
                1 => {
                    exhausted
                        .decode(crate::node::MAX_DECODED_BYTES_PER_GRAPH as usize)
                        .unwrap();
                    (
                        crate::node::DECODED_RESOURCE,
                        crate::node::MAX_DECODED_BYTES_PER_GRAPH,
                    )
                }
                _ => {
                    exhausted
                        .paths(crate::node::MAX_PATH_CHECKS_PER_GRAPH)
                        .unwrap();
                    (
                        crate::node::PATHS_RESOURCE,
                        crate::node::MAX_PATH_CHECKS_PER_GRAPH as u64,
                    )
                }
            };
            assert!(!exhausted.exceedances().is_exceeded());
            assert_eq!(
                namespace
                    .revalidate_inventory(
                        &stored,
                        &root,
                        Mode::Pending,
                        &mut allocation,
                        &exhausted,
                        1024
                    )
                    .unwrap(),
                6
            );
            let record = exhausted.exceedances();
            let axis = record
                .axis(name)
                .expect("the spent axis records its crossing");
            assert_eq!(axis.target, target);
            assert!(axis.high_water > target);
        }
        assert!(namespace
            .revalidate_inventory(&stored, &root, Mode::Pending, &mut allocation, &graph, 5,)
            .is_err());
        fs::write(&archive, b"after!").unwrap();
        stored.replay(&root, Mode::Pending, &graph).unwrap();
        for protected in [&independent, &queried] {
            fs::write(protected, b"after!").unwrap();
            assert!(stored.replay(&root, Mode::Pending, &graph).is_err());
            // A new consumer cannot reuse the previous invocation's read.
            assert!(namespace
                .revalidate_inventory(&stored, &root, Mode::Pending, &mut allocation, &graph, 1024,)
                .is_err());
            fs::write(protected, b"before").unwrap();
        }
        fs::write(&archive, b"larger!").unwrap();
        assert!(stored.replay(&root, Mode::Pending, &graph).is_err());
        fs::write(&archive, b"before").unwrap();
        fs::write(root.join("build/new.rlib"), b"before").unwrap();
        assert!(stored.replay(&root, Mode::Pending, &graph).is_err());
        fs::remove_file(root.join("build/new.rlib")).unwrap();
        fs::remove_file(&archive).unwrap();
        assert!(stored.replay(&root, Mode::Pending, &graph).is_err());
    }

    fn namespace_fixture() -> crate::extract::plan::namespace::Retained {
        use crate::extract::plan::namespace::{Facts, Member, Retained};
        Retained {
            roots: vec!["/bundle/build".into()],
            explicit: Vec::new(),
            members: [
                "plain.rlib",
                "file.rlib",
                "tree/a.rlib",
                "unit.d.rlib",
                "script/output.rlib",
                "rejected.rmeta",
            ]
            .into_iter()
            .map(|name| {
                let selected = name == "rejected.rmeta";
                Member {
                    path: Path::new("/bundle/build").join(name),
                    facts: Facts {
                        kind: 1,
                        mode: 0o100644,
                        length: 4,
                        allocated: 512,
                        device: 1,
                        inode: 1,
                        links: 1,
                    },
                    candidate: true,
                    explicit: false,
                    selected,
                    digest: selected.then_some([0; 32]),
                }
            })
            .collect(),
        }
    }

    #[test]
    fn query_omissions_lose_to_independent_file_tree_depinfo_and_script_roles() {
        use crate::input::proof::Role;
        let budget = GraphBudget::new();
        let mut builder = Builder::bind(Path::new("/bundle"), &budget).unwrap();
        let namespace = namespace_fixture();
        builder.namespace(Some(&namespace)).unwrap();
        builder
            .add(Path::new("/bundle/build"), Kind::Search)
            .unwrap();
        for member in &namespace.members {
            builder.build_output(&member.path).unwrap();
        }
        builder
            .inputs(
                &[
                    Entry::File {
                        role: Role::DependencySource,
                        package_id: None,
                        path: "/bundle/build/file.rlib".into(),
                        bytes: 4,
                        digest: [0; 32],
                    },
                    Entry::Directory {
                        role: Role::DeclaredInput,
                        package_id: None,
                        path: "/bundle/build/tree".into(),
                        directories: Box::default(),
                        files: Box::default(),
                    },
                ],
                &[
                    stored::Recipe::DepInfo {
                        package_id: "unit".into(),
                        path: "/bundle/build/unit.d.rlib".into(),
                        package_root: "/source".into(),
                        out_directories: Box::default(),
                    },
                    stored::Recipe::SavedOutput {
                        package_id: "script".into(),
                        out_dir: "/bundle/build/script/out".into(),
                    },
                ],
            )
            .unwrap();
        let scope = builder.finish().unwrap();
        assert!(!scope.requires_content(Path::new("/bundle/build/plain.rlib")));
        for name in [
            "file.rlib",
            "tree/a.rlib",
            "unit.d.rlib",
            "script/output.rlib",
            "rejected.rmeta",
            "unknown.rmeta",
        ] {
            assert!(
                scope.requires_content(&Path::new("/bundle/build").join(name)),
                "{name}"
            );
        }
        assert!(scope.requires_content(Path::new("/bundle/build/./plain.rlib")));
        assert!(scope.shares_with(&budget));
    }

    #[test]
    fn absent_namespace_cannot_narrow_and_production_builder_keeps_build_role() {
        let budget = GraphBudget::new();
        let mut builder = Builder::bind(Path::new("/bundle"), &budget).unwrap();
        assert!(builder.namespace(None).is_err());
        builder
            .build_output(Path::new("/bundle/build/plain.rlib"))
            .unwrap();
        let scope = builder.finish().unwrap();
        assert!(scope.requires_content(Path::new("/bundle/build/plain.rlib")));
    }

    #[test]
    fn query_ownership_uses_the_spent_original_graph() {
        let budget = GraphBudget::new();
        let mut builder = Builder::bind(Path::new("/bundle"), &budget).unwrap();
        budget
            .decode(usize::try_from(crate::node::MAX_DECODED_BYTES_PER_GRAPH).unwrap())
            .unwrap();
        assert!(!budget.exceedances().is_exceeded());
        // The spent decode account is a target. Narrowing still copies
        // every omitted member and charges the original graph, whose record
        // names the first omitted path as the first charge past the target.
        let namespace = namespace_fixture();
        builder.namespace(Some(&namespace)).unwrap();
        let omitted: Vec<_> = namespace
            .members
            .iter()
            .filter(|member| member.candidate && !member.selected)
            .map(|member| member.path.clone())
            .collect();
        assert_eq!(omitted.len(), 5);
        assert_eq!(builder.omitted.len(), omitted.len());
        let record = budget.exceedances();
        let axis = record
            .axis(crate::node::DECODED_RESOURCE)
            .expect("narrowing charges the spent original graph");
        assert_eq!(axis.target, crate::node::MAX_DECODED_BYTES_PER_GRAPH);
        assert_eq!(
            axis.first_requested,
            axis.target + omitted[0].as_os_str().len() as u64
        );
        assert!(axis.high_water >= axis.first_requested);
        assert!(axis.site.is_some());
    }

    #[test]
    fn search_keeps_all_candidate_groups_and_other_roles_override_it() {
        let budget = GraphBudget::new();
        let root = Path::new("/bundle");
        let mut builder = Builder::bind(root, &budget).unwrap();
        builder
            .add(Path::new("/bundle/build/deps"), Kind::Search)
            .unwrap();
        builder
            .add(Path::new("/bundle/build/deps/required.o"), Kind::File)
            .unwrap();
        builder
            .add(Path::new("/bundle/build/deps/out"), Kind::Tree)
            .unwrap();
        let scope = builder.finish().unwrap();
        for name in [
            "liba-1.rmeta",
            "liba-2.rmeta",
            "liba-3.rlib",
            "other.dylib",
            "liba.rs",
            "required.o",
            "out/arbitrary.o",
        ] {
            assert!(
                scope.requires_content(&root.join("build/deps").join(name)),
                "{name}"
            );
        }
        for name in ["discarded.o", "libdiagnostic.a", "unsearched/nested.rmeta"] {
            assert!(
                !scope.requires_content(&root.join("build/deps").join(name)),
                "{name}"
            );
        }
        assert!(scope.requires_content(Path::new("/outside/unknown")));
        for path in ["/bundle", "/bundle/build", "/bundle/build/deps"] {
            assert!(scope.requires_directory(Path::new(path)), "{path}");
        }
        assert!(!scope.requires_membership(Path::new("/bundle/build")));
        assert!(scope.requires_membership(Path::new("/bundle/build/deps")));
        assert!(scope.requires_membership(Path::new("/bundle/build/deps/out/nested")));
        assert!(!scope.requires_directory(Path::new("/bundle/build/unrelated")));
        assert!(!scope.requires_membership(Path::new("/bundle/build/deps/unsearched")));
        assert!(scope.requires_directory(Path::new("/outside/unknown")));
    }

    #[test]
    fn duplicate_roles_keep_the_strongest_obligation() {
        let budget = GraphBudget::new();
        let mut builder = Builder::bind(Path::new("/bundle"), &budget).unwrap();
        for kind in [Kind::Tree, Kind::File, Kind::Search, Kind::File] {
            builder.add(Path::new("/bundle/shared"), kind).unwrap();
        }
        let scope = builder.finish().unwrap();
        assert_eq!(scope.obligations.len(), 1);
        assert!(scope.requires_content(Path::new("/bundle/shared/nested/anything")));
    }

    #[test]
    fn input_and_script_obligations_cover_generated_and_negative_paths() {
        use crate::input::proof::Role;
        let budget = GraphBudget::new();
        let mut builder = Builder::bind(Path::new("/bundle"), &budget).unwrap();
        builder
            .inputs(
                &[
                    Entry::File {
                        role: Role::DependencySource,
                        package_id: None,
                        path: "/bundle/build/generated.o".into(),
                        bytes: 0,
                        digest: [0; 32],
                    },
                    Entry::Missing {
                        role: Role::DeclaredInput,
                        package_id: None,
                        path: "/bundle/build/optional".into(),
                    },
                    Entry::Directory {
                        role: Role::DeclaredInput,
                        package_id: None,
                        path: "/bundle/declared".into(),
                        directories: Box::default(),
                        files: Box::default(),
                    },
                ],
                &[
                    stored::Recipe::SavedOutput {
                        package_id: "script".into(),
                        out_dir: "/bundle/build/script/out".into(),
                    },
                    stored::Recipe::DepInfo {
                        package_id: "unit".into(),
                        path: "/bundle/build/unit.d".into(),
                        package_root: "/source".into(),
                        out_directories: vec![PathBuf::from("/bundle/build/generated")]
                            .into_boxed_slice(),
                    },
                ],
            )
            .unwrap();
        let scope = builder.finish().unwrap();
        for path in [
            "build/generated.o",
            "build/optional",
            "declared/deep/file",
            "build/script/output",
            "build/script/out/arbitrary.o",
            "build/unit.d",
            "build/generated/file",
        ] {
            assert!(
                scope.requires_content(&Path::new("/bundle").join(path)),
                "{path}"
            );
        }
        assert!(!scope.requires_content(Path::new("/bundle/build/unobserved")));
    }

    #[test]
    fn enclosing_declared_tree_covers_the_allocation() {
        let budget = GraphBudget::new();
        let mut builder = Builder::bind(Path::new("/declared/bundle"), &budget).unwrap();
        builder.add(Path::new("/declared"), Kind::Tree).unwrap();
        let scope = builder.finish().unwrap();
        assert!(scope.requires_content(Path::new("/declared/bundle/any/file.o")));
        assert!(scope.shares_with(&budget));
        assert!(!scope.shares_with(&GraphBudget::new()));
    }

    #[test]
    fn selection_refuses_paths_and_records_exceedances_before_ownership() {
        let budget = GraphBudget::new();
        let mut builder = Builder::bind(Path::new("/bundle"), &budget).unwrap();
        assert!(matches!(
            builder.add(Path::new("/bundle/../escape"), Kind::File),
            Err(Error::OutsideRoot(_))
        ));
        assert_eq!(builder.obligations.len(), 0);
        budget
            .decode(usize::try_from(crate::node::MAX_DECODED_BYTES_PER_GRAPH).unwrap())
            .unwrap();
        assert!(!budget.exceedances().is_exceeded());
        // The spent decode account is a target, not a gate: the path is
        // still retained and the owned copy is the first charge past the target.
        let path = Path::new("/bundle/file");
        builder.add(path, Kind::File).unwrap();
        assert_eq!(builder.obligations.len(), 1);
        let record = budget.exceedances();
        let axis = record
            .axis(crate::node::DECODED_RESOURCE)
            .expect("selection charges the original graph");
        assert_eq!(axis.target, crate::node::MAX_DECODED_BYTES_PER_GRAPH);
        assert_eq!(
            axis.first_requested,
            axis.target + path.as_os_str().len() as u64
        );
        assert!(axis.site.is_some());
    }
}

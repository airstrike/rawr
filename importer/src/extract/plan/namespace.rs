//! Observations of caller-supplied roots, not proof that those roots are complete.
//! Selector, argv and compiler coverage authority belong to the calling owner.

use std::fs;
use std::os::unix::fs::MetadataExt as _;
use std::path::{Component, Path, PathBuf};

use super::candidate;
use crate::extract::{protocol::attempt::Completion, Allocation};
use crate::node::GraphBudget;
use crate::runner::artifact::retained;
use crate::store::bundle;
use crate::{Error, Result};

pub(crate) struct Snapshot {
    retained: Retained,
    graph: GraphBudget,
}

pub(crate) struct Retained {
    pub(crate) roots: Vec<PathBuf>,
    pub(crate) explicit: Vec<PathBuf>,
    pub(crate) members: Vec<Member>,
}

pub(crate) struct Member {
    pub(crate) path: PathBuf,
    pub(crate) facts: Facts,
    pub(crate) candidate: bool,
    pub(crate) explicit: bool,
    pub(crate) digest: Option<[u8; 32]>,
    pub(crate) selected: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct Facts {
    pub(crate) kind: u8,
    pub(crate) mode: u32,
    pub(crate) length: u64,
    pub(crate) allocated: u64,
    pub(crate) device: u64,
    pub(crate) inode: u64,
    pub(crate) links: u64,
}

impl Facts {
    /// Whether `current` still satisfies these recorded facts. Logical
    /// geometry and identity are exact; the filesystem may release allocated
    /// blocks after a file is closed, so allocation may fall but never grow
    /// (the rule for sealed geometry).
    fn holds(self, current: Self) -> bool {
        current.allocated <= self.allocated
            && Self {
                allocated: self.allocated,
                ..current
            } == self
    }
}

impl Snapshot {
    pub(crate) fn capture(
        roots: &[PathBuf],
        explicit: &[PathBuf],
        allocation: &mut Allocation,
        graph: &GraphBudget,
    ) -> Result<Self> {
        let members = observe(roots, explicit, allocation, true)?;
        Ok(Self {
            retained: Retained {
                roots: own_paths(roots, allocation)?,
                explicit: own_paths(explicit, allocation)?,
                members,
            },
            graph: graph.clone(),
        })
    }

    pub(crate) fn complete(
        mut self,
        completion: &Completion<'_>,
        allocation: &mut Allocation,
        graph: &GraphBudget,
    ) -> Result<Retained> {
        if !self.graph.shares_with(graph) {
            return Err(invalid("namespace completion belongs to another graph"));
        }
        let current = observe(
            &self.retained.roots,
            &self.retained.explicit,
            allocation,
            false,
        )?;
        compare_members(&self.retained.members, &current, None)?;
        let count = current.iter().filter(|member| member.candidate).count();
        let mut candidates = Vec::new();
        allocation.items(count)?;
        candidates
            .try_reserve_exact(count)
            .map_err(|_| node(crate::node::Error::Allocation))?;
        for member in current.iter().filter(|member| member.candidate) {
            candidates.push(candidate::Candidate {
                original: &member.path,
                canonical: &member.path,
            });
        }
        let joined = candidate::join(&candidates, completion, allocation)?;
        let mut flags = joined.flags.iter();
        for member in &mut self.retained.members {
            if member.candidate {
                let flag = flags
                    .next()
                    .ok_or_else(|| invalid("namespace association rows absent"))?;
                let rlib = member
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(".rlib"));
                member.selected = member.explicit
                    || joined.conservative
                    || if rlib { flag.attempt } else { flag.query };
            }
            if !member.selected {
                member.digest = None;
            }
        }
        verify_content(&self.retained.members, allocation)?;
        Ok(self.retained)
    }
}

impl Retained {
    /// Validate both complete memberships and share only agreeing fresh content
    /// reads within this invocation. Returned bytes are accounting, not authority.
    #[cfg(all(test, feature = "rustc-driver"))]
    pub(crate) fn revalidate_inventory(
        &self,
        inventory: &retained::Stored,
        root: &Path,
        mode: retained::Mode,
        allocation: &mut Allocation,
        graph: &GraphBudget,
    ) -> Result<u64> {
        self.revalidate_inventory_from(
            inventory,
            root,
            retained::Origin::Unpublished(mode),
            allocation,
            graph,
        )
    }

    #[cfg(all(test, feature = "rustc-driver"))]
    pub(crate) fn revalidate_inventory_from(
        &self,
        inventory: &retained::Stored,
        root: &Path,
        origin: retained::Origin<'_>,
        allocation: &mut Allocation,
        graph: &GraphBudget,
    ) -> Result<u64> {
        self.revalidate_inventory_scoped_from(
            inventory,
            root,
            origin,
            crate::node::read_set::Scope::Complete,
            allocation,
            graph,
        )
    }

    #[cfg(all(test, feature = "rustc-driver"))]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn revalidate_inventory_scoped_from(
        &self,
        inventory: &retained::Stored,
        root: &Path,
        origin: retained::Origin<'_>,
        scope: crate::node::read_set::Scope,
        allocation: &mut Allocation,
        graph: &GraphBudget,
    ) -> Result<u64> {
        self.revalidate_inventory_completing(
            inventory, root, origin, scope, None, allocation, graph,
        )
    }

    #[cfg(feature = "rustc-driver")]
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn revalidate_inventory_completing(
        &self,
        inventory: &retained::Stored,
        root: &Path,
        origin: retained::Origin<'_>,
        scope: crate::node::read_set::Scope,
        completion: Option<bundle::Completion<'_>>,
        allocation: &mut Allocation,
        graph: &GraphBudget,
    ) -> Result<u64> {
        // A frozen bundle root is the one known local permission transition;
        // the shared historical expectation is converted per replay, never
        // rewritten, so one decoded namespace serves every mode.
        let frozen = matches!(origin.mode(), retained::Mode::Frozen).then_some(root);
        let current = observe(&self.roots, &self.explicit, allocation, false)?;
        compare_members(&self.members, &current, frozen)?;
        allocation.items(self.members.len())?;
        let mut shared = Vec::new();
        shared
            .try_reserve_exact(self.members.len())
            .map_err(|_| node(crate::node::Error::Allocation))?;
        shared.resize(self.members.len(), 0_u8);
        let mut bytes = 0_u64;
        retained::observe_hashes(|hashes| {
            inventory.replay_resolution_content_completing(
                root,
                origin,
                graph,
                scope,
                completion,
                |path, metadata, expected| {
                    let member = self
                        .members
                        .binary_search_by(|member| member.path.as_os_str().cmp(path.as_os_str()))
                        .ok()
                        .filter(|index| self.members[*index].selected);
                    if let Some(index) = member {
                        let member = &self.members[index];
                        if member.digest != Some(expected)
                            || !expected_facts(member, frozen).holds(
                                facts(&member.path, metadata).map_err(|_| {
                                    crate::node::Error::Codec(
                                        "joint namespace physical facts invalid",
                                    )
                                })?,
                            )
                        {
                            return Err(crate::node::Error::Codec(
                                "joint inventory/namespace obligations disagree",
                            ));
                        }
                    }
                    let actual = hashes.hash(path, metadata)?;
                    if actual == expected
                        && let Some(index) = member
                    {
                        shared[index] = 1;
                        bytes = bytes
                            .checked_add(metadata.len())
                            .ok_or(crate::node::Error::Allocation)?;
                    }
                    Ok(actual)
                },
            )
        })
        .map_err(node)?;
        verify_content_with(&self.members, frozen, allocation, |index| {
            shared[index] != 0
        })?;
        Ok(bytes)
    }

    pub(crate) fn require_selection(
        &self,
        completion: &Completion<'_>,
        allocation: &mut Allocation,
    ) -> Result<()> {
        let count = self
            .members
            .iter()
            .filter(|member| member.candidate)
            .count();
        allocation.items(count)?;
        let mut candidates = Vec::new();
        candidates
            .try_reserve_exact(count)
            .map_err(|_| node(crate::node::Error::Allocation))?;
        for member in self.members.iter().filter(|member| member.candidate) {
            candidates.push(candidate::Candidate {
                original: &member.path,
                canonical: &member.path,
            });
        }
        let joined = candidate::join(&candidates, completion, allocation)?;
        let mut flags = joined.flags.iter();
        for member in &self.members {
            let expected = if member.candidate {
                let flag = flags
                    .next()
                    .ok_or_else(|| invalid("namespace selection absent"))?;
                let rlib = member
                    .path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.ends_with(".rlib"));
                member.explicit
                    || joined.conservative
                    || if rlib { flag.attempt } else { flag.query }
            } else {
                false
            };
            if member.selected != expected || member.digest.is_some() != expected {
                return Err(invalid(
                    "retained selection differs from compiler completion",
                ));
            }
        }
        Ok(())
    }

    pub(crate) fn revalidate(&self, allocation: &mut Allocation) -> Result<()> {
        let current = observe(&self.roots, &self.explicit, allocation, false)?;
        compare_members(&self.members, &current, None)?;
        verify_content(&self.members, allocation)
    }
}

/// Interpret only the known local bundle permission transition for members under
/// a frozen root. This is historical expectation conversion; the caller must
/// still observe the current facts.
fn expected_facts(member: &Member, frozen: Option<&Path>) -> Facts {
    let mut facts = member.facts;
    if frozen.is_some_and(|root| member.path.starts_with(root)) {
        // As `store::bundle::frozen_mode`: directories stay owner-writable.
        let permissions = if facts.kind == 0 {
            0o755
        } else if facts.mode & 0o111 != 0 {
            0o555
        } else {
            0o444
        };
        facts.mode = (facts.mode & !0o7777) | permissions;
    }
    facts
}

fn compare_members(expected: &[Member], current: &[Member], frozen: Option<&Path>) -> Result<()> {
    if expected.len() != current.len()
        || expected.iter().zip(current).any(|(left, right)| {
            left.path.as_os_str() != right.path.as_os_str()
                || !expected_facts(left, frozen).holds(right.facts)
                || left.candidate != right.candidate
                || left.explicit != right.explicit
        })
    {
        return Err(invalid("namespace membership or physical facts changed"));
    }
    Ok(())
}

fn verify_content(members: &[Member], allocation: &mut Allocation) -> Result<()> {
    verify_content_with(members, None, allocation, |_| false)
}

fn verify_content_with(
    members: &[Member],
    frozen: Option<&Path>,
    allocation: &mut Allocation,
    shared: impl Fn(usize) -> bool,
) -> Result<()> {
    // Keep namespace errors intact while completing every opened attachment
    // before this consumer returns either its result or an error.
    retained::observe_hashes(|hashes| {
        Ok((|| {
            for (index, member) in members
                .iter()
                .enumerate()
                .filter(|(_, member)| member.selected)
            {
                let metadata = metadata(&member.path, allocation)?;
                if !expected_facts(member, frozen).holds(facts(&member.path, &metadata)?) {
                    return Err(invalid("selected namespace physical facts changed"));
                }
                if shared(index) {
                    continue;
                }
                let digest = hashes.hash(&member.path, &metadata).map_err(node)?;
                if Some(digest) != member.digest {
                    return Err(invalid("selected namespace content changed"));
                }
            }
            Ok(())
        })())
    })
    .map_err(node)?
}

fn observe(
    roots: &[PathBuf],
    explicit: &[PathBuf],
    allocation: &mut Allocation,
    hash: bool,
) -> Result<Vec<Member>> {
    bounded(
        roots
            .len()
            .checked_add(explicit.len())
            .ok_or_else(|| node(crate::node::Error::Allocation))?,
    )?;
    let mut members = bundle::buffer::Buffer::new();
    for root in roots {
        require_path(root)?;
        let canonical = retained::directory(root).map_err(node)?;
        if canonical.as_os_str() != root.as_os_str() {
            return Err(invalid(
                "namespace root is not its exact canonical directory",
            ));
        }
        let path = own_path(root, allocation)?;
        push_member(&mut members, path, false, allocation)?;
        // The reviewed primitive owns temporary native names under GraphBudget.
        // Retained paths/rows are separately admitted to Extraction Allocation.
        let names = bundle::names(
            root,
            (bundle::MAX_MEMBERS as usize).saturating_sub(members.len()),
        )
        .map_err(node)?;
        for name in names {
            let maximum = root
                .as_os_str()
                .len()
                .checked_add(1)
                .and_then(|n| n.checked_add(name.len()))
                .ok_or_else(|| node(crate::node::Error::Allocation))?;
            allocation.path_capacity(maximum)?;
            let path = bundle::path(root, Path::new(&name)).map_err(node)?;
            push_member(&mut members, path, false, allocation)?;
        }
    }
    for path in explicit {
        require_path(path)?;
        let parent = path
            .parent()
            .ok_or_else(|| invalid("explicit candidate has no parent"))?;
        if retained::directory(parent).map_err(node)?.as_os_str() != parent.as_os_str() {
            return Err(invalid("explicit candidate parent is not canonical"));
        }
        let path = own_path(path, allocation)?;
        push_member(&mut members, path, true, allocation)?;
    }
    let mut members = members.into_vec();
    let observed_rows = members.len();
    members.sort_unstable_by(|left, right| left.path.as_os_str().cmp(right.path.as_os_str()));
    for pair in members.windows(2) {
        if pair[0].path.as_os_str() == pair[1].path.as_os_str() && pair[0].facts != pair[1].facts {
            return Err(invalid("overlapping namespace observations changed"));
        }
    }
    members.dedup_by(|later, earlier| {
        if later.path.as_os_str() != earlier.path.as_os_str() {
            return false;
        }
        earlier.explicit |= later.explicit;
        earlier.candidate |= later.candidate;
        true
    });
    if option_env!("RAWR_SCOPE_CENSUS") == Some("1") {
        let unique_roots = roots
            .iter()
            .enumerate()
            .filter(|(index, path)| !roots[..*index].contains(path))
            .count();
        eprintln!(
            "rawr-namespace-overlap-v1\tinitial={}\troots={}\tunique_roots={}\texplicit={}\tobserved_rows={}\tunique_members={}",
            hash, roots.len(), unique_roots, explicit.len(), observed_rows, members.len()
        );
    }
    if hash {
        // This batch ends before the compiler starts; completion opens its own
        // fresh batch and cannot reuse these directory observations.
        retained::observe_hashes(|hashes| {
            Ok((|| {
                for member in members.iter_mut().filter(|member| member.candidate) {
                    let metadata = metadata(&member.path, allocation)?;
                    if !member.facts.holds(facts(&member.path, &metadata)?) {
                        return Err(invalid("namespace physical facts changed before hashing"));
                    }
                    member.digest = Some(hashes.hash(&member.path, &metadata).map_err(node)?);
                }
                Ok(())
            })())
        })
        .map_err(node)??;
    }
    Ok(members)
}

fn push_member(
    members: &mut bundle::buffer::Buffer<Member>,
    path: PathBuf,
    explicit: bool,
    allocation: &mut Allocation,
) -> Result<()> {
    if !explicit && crate::input::split_debuginfo(&path) {
        // Packed debug info (`.dSYM`, which Cargo links beside a build
        // script) is never a crate rustc can find, and never read.
        return Ok(());
    }
    bounded(members.len().saturating_add(1))?;
    let metadata = metadata(&path, allocation)?;
    let facts = facts(&path, &metadata)?;
    if explicit && facts.kind != 1 {
        return Err(invalid("explicit candidate is not a file"));
    }
    let candidate = facts.kind == 1 && (explicit || candidate_name(&path));
    allocation.items(1)?;
    members
        .push(Member {
            path,
            facts,
            candidate,
            explicit,
            digest: None,
            selected: false,
        })
        .map_err(node)
}

fn candidate_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_none_or(|name| {
            [".rlib", ".rmeta", ".dylib", ".so", ".rs"]
                .iter()
                .any(|suffix| name.ends_with(suffix))
        })
}

fn metadata(path: &Path, allocation: &mut Allocation) -> Result<fs::Metadata> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata),
        Err(source) => {
            let path = own_path(path, allocation)?;
            Err(Error::Io {
                operation: "inspect compiler namespace",
                path,
                source,
            })
        }
    }
}

fn facts(path: &Path, metadata: &fs::Metadata) -> Result<Facts> {
    if metadata.file_type().is_symlink()
        || !(metadata.is_dir() || metadata.is_file())
        || metadata.is_file() && bundle::hard_linked(metadata)
    {
        return Err(Error::Extraction {
            code: "compiler-namespace",
            reason: format!(
                "namespace member `{}` has unsupported type or links",
                path.display()
            )
            .into(),
        });
    }
    Ok(Facts {
        kind: if metadata.is_dir() { 0 } else { 1 },
        mode: metadata.mode(),
        length: metadata.len(),
        allocated: metadata
            .blocks()
            .checked_mul(512)
            .ok_or_else(|| node(crate::node::Error::Allocation))?,
        device: metadata.dev(),
        inode: metadata.ino(),
        links: metadata.nlink(),
    })
}

fn own_path(path: &Path, allocation: &mut Allocation) -> Result<PathBuf> {
    allocation.path(path)?;
    let mut owned = PathBuf::new();
    owned
        .try_reserve_exact(path.as_os_str().len())
        .map_err(|_| node(crate::node::Error::Allocation))?;
    owned.push(path);
    Ok(owned)
}

fn own_paths(paths: &[PathBuf], allocation: &mut Allocation) -> Result<Vec<PathBuf>> {
    allocation.items(paths.len())?;
    let mut owned = Vec::new();
    owned
        .try_reserve_exact(paths.len())
        .map_err(|_| node(crate::node::Error::Allocation))?;
    for path in paths {
        owned.push(own_path(path, allocation)?);
    }
    Ok(owned)
}

fn require_path(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| matches!(part, Component::ParentDir))
    {
        return Err(invalid("namespace path is relative or traverses a parent"));
    }
    Ok(())
}
fn bounded(count: usize) -> Result<()> {
    if count > bundle::MAX_MEMBERS as usize {
        return Err(Error::Limit {
            resource: "compiler namespace members",
            limit: bundle::MAX_MEMBERS as usize,
            actual: count,
        });
    }
    Ok(())
}
fn invalid(reason: &'static str) -> Error {
    Error::Extraction {
        code: "compiler-namespace",
        reason: reason.into(),
    }
}
fn node(source: crate::node::Error) -> Error {
    Error::Node {
        operation: "compiler namespace",
        source: Box::new(source),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::protocol::attempt::{self, Invocation};
    use std::sync::atomic::{AtomicUsize, Ordering};
    static NEXT: AtomicUsize = AtomicUsize::new(0);

    /// Give every regular file under `root` a new status-change time, so its
    /// physical identity, and therefore its memoized digest, changes.
    fn touch_files(root: &Path) {
        for child in fs::read_dir(root).unwrap() {
            let path = child.unwrap().path();
            let metadata = fs::symlink_metadata(&path).unwrap();
            if metadata.is_dir() {
                touch_files(&path);
            } else if metadata.is_file() {
                fs::set_permissions(&path, metadata.permissions()).unwrap();
            }
        }
    }

    #[test]
    fn recorded_facts_allow_released_blocks_but_nothing_else() {
        let recorded = Facts {
            kind: 1,
            mode: 0o100_444,
            length: 10_000,
            allocated: 16_384,
            device: 1,
            inode: 2,
            links: 1,
        };
        assert!(recorded.holds(recorded));
        assert!(recorded.holds(Facts {
            allocated: 12_288,
            ..recorded
        }));
        assert!(!recorded.holds(Facts {
            allocated: 20_480,
            ..recorded
        }));
        assert!(!recorded.holds(Facts {
            length: 10_001,
            ..recorded
        }));
        assert!(!recorded.holds(Facts {
            inode: 3,
            ..recorded
        }));
        assert!(!recorded.holds(Facts {
            mode: 0o100_644,
            ..recorded
        }));
    }

    const ID: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "rawr-namespace-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&path).unwrap();
            let root = fs::canonicalize(path).unwrap();
            fs::write(root.join("liba.rmeta"), b"meta").unwrap();
            fs::write(root.join("liba.rlib"), b"archive").unwrap();
            fs::write(root.join("opaque"), b"opaque").unwrap();
            Self(root)
        }
        fn roots(&self) -> Vec<PathBuf> {
            vec![self.0.clone()]
        }
        fn completion(&self, conservative: bool) -> Vec<u8> {
            let query = self.0.join("liba.rmeta");
            let mut bytes = Vec::new();
            attempt::encode(
                &mut bytes,
                ID,
                conservative,
                &[Invocation {
                    nonce: ID,
                    correlation: ID,
                }],
                std::iter::empty(),
                [query.to_str().unwrap()].into_iter(),
            )
            .unwrap();
            bytes
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn allocation() -> Allocation {
        Allocation::with_limits(usize::MAX, usize::MAX)
    }

    #[test]
    fn selected_content_and_full_physical_membership_remain_current() {
        let fixture = Fixture::new();
        let graph = GraphBudget::new();
        let mut allocation = allocation();
        let snapshot = Snapshot::capture(&fixture.roots(), &[], &mut allocation, &graph).unwrap();
        // Unattempted archive bytes may differ, but all physical facts must match.
        fs::write(fixture.0.join("liba.rlib"), b"changed").unwrap();
        let bytes = fixture.completion(false);
        let retained = snapshot
            .complete(
                &Completion::decode(&bytes).unwrap(),
                &mut allocation,
                &graph,
            )
            .unwrap();
        retained.revalidate(&mut allocation).unwrap();
        fs::write(fixture.0.join("liba.rmeta"), b"META").unwrap();
        // A successful earlier read plus unchanged physical facts cannot serve
        // the later consumer: an in-place content edit preserves this identity.
        let current = observe(&retained.roots, &retained.explicit, &mut allocation, false).unwrap();
        compare_members(&retained.members, &current, None).unwrap();
        assert!(retained.revalidate(&mut allocation).is_err());
        fs::write(fixture.0.join("liba.rmeta"), b"meta").unwrap();
        fs::write(fixture.0.join("opaque"), b"growth!").unwrap();
        assert!(retained.revalidate(&mut allocation).is_err());
    }

    #[test]
    fn overlap_deduplicates_and_explicit_role_wins() {
        let fixture = Fixture::new();
        let graph = GraphBudget::new();
        let mut allocation = allocation();
        let explicit = [fixture.0.join("liba.rlib"), fixture.0.join("opaque")];
        let snapshot = Snapshot::capture(
            &[fixture.0.clone(), fixture.0.clone()],
            &explicit,
            &mut allocation,
            &graph,
        )
        .unwrap();
        assert_eq!(snapshot.retained.roots.len(), 2);
        assert_eq!(snapshot.retained.members.len(), 4);
        let bytes = fixture.completion(false);
        let retained = snapshot
            .complete(
                &Completion::decode(&bytes).unwrap(),
                &mut allocation,
                &graph,
            )
            .unwrap();
        assert!(retained
            .members
            .iter()
            .filter(|member| member.explicit)
            .all(|member| member.selected));
        fs::write(&explicit[0], b"changed").unwrap();
        assert!(retained.revalidate(&mut allocation).is_err());
    }

    #[test]
    fn overlap_prehasher_reads_each_unique_candidate_once() {
        let fixture = Fixture::new();
        let graph = GraphBudget::new();
        let mut allocation = allocation();
        let snapshot = Snapshot::capture(
            &[fixture.0.clone(), fixture.0.clone()],
            &[fixture.0.join("liba.rmeta"), fixture.0.join("opaque")],
            &mut allocation,
            &graph,
        )
        .unwrap();
        assert_eq!(snapshot.retained.members.len(), 4);
    }

    #[test]
    fn sibling_hashing_reduces_paths_and_reuses_unchanged_content_between_phases() {
        let fixture = Fixture::new();
        for index in 0..32 {
            fs::write(fixture.0.join(format!("sibling{index}.rmeta")), b"first").unwrap();
        }
        let mut legacy_allocation = allocation();
        let legacy_members = observe(&fixture.roots(), &[], &mut legacy_allocation, false).unwrap();
        for member in legacy_members.iter().filter(|member| member.candidate) {
            let metadata = metadata(&member.path, &mut legacy_allocation).unwrap();
            retained::hash(&member.path, &metadata).unwrap();
        }
        let mut allocation = allocation();
        let mut members = observe(&fixture.roots(), &[], &mut allocation, true).unwrap();

        for member in &mut members {
            member.selected = member.candidate;
        }
        for member in members.iter().filter(|member| member.selected) {
            let metadata = metadata(&member.path, &mut legacy_allocation).unwrap();
            retained::hash(&member.path, &metadata).unwrap();
        }
        verify_content(&members, &mut allocation).unwrap();
        // A changed identity is read and hashed again in the later phase.
        touch_files(&fixture.0);
        verify_content(&members, &mut allocation).unwrap();

        fs::write(fixture.0.join("sibling0.rmeta"), b"later").unwrap();
        assert!(matches!(
            verify_content(&members, &mut allocation),
            Err(Error::Extraction { code: "compiler-namespace", reason })
                if reason.as_ref() == "selected namespace content changed"
        ));
    }

    #[test]
    fn metadata_error_ownership_uses_the_original_allocation() {
        let absent = Path::new("/rawr-absent-namespace-diagnostic");
        // Owning the diagnostic path charges the original account, which
        // reports the I/O failure.
        let mut recorded = Allocation::with_limits(usize::MAX, usize::MAX);
        assert!(matches!(
            metadata(absent, &mut recorded).unwrap_err(),
            Error::Io {
                operation: "inspect compiler namespace",
                ..
            }
        ));
        assert!(!recorded.is_refused());
        // Only a genuinely latched original account replaces the diagnostic
        // with its own refusal.
        let error = metadata(absent, &mut Allocation::refused()).unwrap_err();
        assert!(matches!(
            error,
            Error::Node { operation, .. }
                if operation == crate::extract::REFUSED_ACCOUNT_OPERATION
        ));
    }

    #[test]
    fn before_after_drift_and_missing_members_reject() {
        let fixture = Fixture::new();
        let graph = GraphBudget::new();
        let mut allocation = allocation();
        let snapshot = Snapshot::capture(&fixture.roots(), &[], &mut allocation, &graph).unwrap();
        let bytes = fixture.completion(false);
        fs::write(fixture.0.join("liba.rmeta"), b"META").unwrap();
        assert!(snapshot
            .complete(
                &Completion::decode(&bytes).unwrap(),
                &mut allocation,
                &graph
            )
            .is_err());
        let snapshot = Snapshot::capture(&fixture.roots(), &[], &mut allocation, &graph).unwrap();
        fs::remove_file(fixture.0.join("opaque")).unwrap();
        assert!(snapshot
            .complete(
                &Completion::decode(&bytes).unwrap(),
                &mut allocation,
                &graph
            )
            .is_err());
    }

    #[test]
    fn unsafe_shapes_and_original_account_refusal_reject() {
        let fixture = Fixture::new();
        let graph = GraphBudget::new();
        let mut allocation = allocation();
        std::os::unix::fs::symlink(fixture.0.join("opaque"), fixture.0.join("link")).unwrap();
        assert!(Snapshot::capture(&fixture.roots(), &[], &mut allocation, &graph).is_err());
        fs::remove_file(fixture.0.join("link")).unwrap();
        fs::hard_link(fixture.0.join("opaque"), fixture.0.join("link")).unwrap();
        assert!(Snapshot::capture(&fixture.roots(), &[], &mut allocation, &graph).is_err());
        fs::remove_file(fixture.0.join("link")).unwrap();
        // Only a genuinely latched account rejects the capture before it owns
        // any path.
        assert!(matches!(
            Snapshot::capture(
                &fixture.roots(),
                &[],
                &mut Allocation::refused(),
                &GraphBudget::new(),
            ),
            Err(Error::Node { operation, .. })
                if operation == crate::extract::REFUSED_ACCOUNT_OPERATION
        ));
        let snapshot = Snapshot::capture(&fixture.roots(), &[], &mut allocation, &graph).unwrap();
        let bytes = fixture.completion(false);
        assert!(snapshot
            .complete(
                &Completion::decode(&bytes).unwrap(),
                &mut allocation,
                &GraphBudget::new(),
            )
            .is_err());
    }
}

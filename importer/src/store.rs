//! Versioned durable storage for typed importer nodes.

use std::fs;
use std::path::{Path, PathBuf};

use sha2::Digest as _;

use crate::node::codec::{Envelope, MAX_ENVELOPE_BYTES};
use crate::node::read_set::Entry;
use crate::node::replay::Stored;
use crate::node::{
    content_digest, content_digest_parts, ActionKey, Digest, Direct, Error, Kind, Value,
};

mod accounting;
mod batch;
pub(crate) use batch::Batch;
pub(crate) mod bundle;
mod control;
mod entry;
mod eviction;
mod index;
mod lease;
pub(crate) use lease::Ownership;
pub(crate) mod path;
mod pins;
mod recovery;

pub(crate) use entry::{existing_directory_budgeted, read_file_budgeted};

pub(crate) use pins::{transaction, Guard as PinGuard, Role as PinRole};

#[derive(Clone, Copy)]
enum InFlight<'a> {
    Current(&'a crate::node::ReadSetProof),
    Retained(&'a crate::node::read_set::Retention),
}

impl InFlight<'_> {
    fn digest(self) -> Digest {
        match self {
            Self::Current(proof) => proof.digest(),
            Self::Retained(retention) => retention.digest(),
        }
    }
}

pub(crate) const FIXED_DIRECTORIES: &[&str] = &[
    "actions",
    "content",
    "executions",
    "indexes",
    "ledgers",
    "ledgers/projects",
    "locks",
    "locks/actions",
    "locks/executions",
    "locks/projects",
    "locks/owners",
    "pins",
    "pins/live",
    "pins/last-good",
    "pins/prepared",
    "pins/in-flight",
    "temporaries",
];

/// Rawr's home: `$RAWR_HOME` when set, otherwise `<cargo_home>/rawr`. It holds
/// what Rawr shares across projects: rawr-iced's hosts (`hosts`), rawr's
/// compiled runtime (`runtime`), shared bindings (`bindings`), lone scripts'
/// packages (`scripts`) and file digests. A project's own products live in
/// its target directory (D608.4).
#[must_use]
pub fn home(cargo_home: &Path) -> PathBuf {
    if let Some(home) = std::env::var_os("RAWR_HOME") {
        return home.into();
    }
    match TEST_HOME.get() {
        Some(test) if is_default_cargo_home(cargo_home) => test.clone(),
        _ => cargo_home.join("rawr"),
    }
}

/// A test process's stand-in for the real default home; fixture Cargo homes
/// keep their own.
static TEST_HOME: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();

fn is_default_cargo_home(cargo_home: &Path) -> bool {
    let default = std::env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".cargo")));
    default.is_some_and(|default| {
        fs::canonicalize(&default).unwrap_or(default)
            == fs::canonicalize(cargo_home).unwrap_or_else(|_| cargo_home.to_path_buf())
    })
}

/// Keep this test process out of the real store: the default Cargo home's Rawr
/// home becomes `<workspace>/target/test-store/rawr`, one store every test
/// process shares and reuses, as they would share the real one.
#[doc(hidden)]
pub fn isolate_for_tests() -> PathBuf {
    TEST_HOME
        .get_or_init(|| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .expect("the importer lives in the workspace")
                .join("target/test-store/rawr")
        })
        .clone()
}

/// One test's turn on this process's native Cargo lane.
///
/// The lane admits one owner per process and refuses a second outright
/// ("process busy") rather than waiting, so tests sharing a binary hold a turn
/// for their whole body and wait for each other instead.
#[doc(hidden)]
pub fn test_turn() -> std::sync::MutexGuard<'static, ()> {
    static TURN: std::sync::Mutex<()> = std::sync::Mutex::new(());
    TURN.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// One test's project directory under `/tmp`: outside `$HOME`, so no ancestor
/// Cargo config or workspace applies, and short enough for runner sockets.
/// Dropping it removes the tree, pass or fail.
#[doc(hidden)]
pub struct TestProject(PathBuf);

impl TestProject {
    pub fn new(label: &str) -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        #[cfg(unix)]
        let parent = PathBuf::from("/tmp");
        #[cfg(not(unix))]
        let parent = std::env::temp_dir();
        let root = parent.join(format!(
            "{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        remove_test_tree(&root);
        fs::create_dir_all(&root).expect("the test project directory is created");
        Self(fs::canonicalize(root).expect("the test project directory is canonical"))
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TestProject {
    fn drop(&mut self) {
        remove_test_tree(&self.0);
    }
}

/// Remove a test tree, first making sealed read-only directories writable.
fn remove_test_tree(root: &Path) {
    let _ = remove_tree(root);
}

/// Remove a tree whose sealed directories may be read-only.
fn remove_tree(root: &Path) -> std::io::Result<()> {
    fn thaw(path: &Path) {
        let Ok(metadata) = fs::symlink_metadata(path) else {
            return;
        };
        if !metadata.is_dir() {
            return;
        }
        let mut permissions = metadata.permissions();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt as _;
            permissions.set_mode(permissions.mode() | 0o700);
        }
        #[cfg(not(unix))]
        #[allow(clippy::permissions_set_readonly_false, reason = "removal")]
        permissions.set_readonly(false);
        let _ = fs::set_permissions(path, permissions);
        for entry in fs::read_dir(path).into_iter().flatten().flatten() {
            thaw(&entry.path());
        }
    }
    match fs::remove_dir_all(root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => {
            thaw(root);
            fs::remove_dir_all(root)
        }
        Ok(()) => Ok(()),
    }
}

/// Empty a store root another format wrote, so it bootstraps fresh.
fn clear(root: &Path) -> Result<(), Error> {
    remove_tree(root)
        .and_then(|()| fs::create_dir(root))
        .map_err(|source| Error::Io {
            operation: "clear a typed store of another format",
            path: root.to_path_buf(),
            source,
        })
}

/// One canonical versioned typed-node store. Clones share its graph budget.
#[derive(Clone)]
pub struct Store {
    root: PathBuf,
    project: Digest,
    graph_budget: crate::node::GraphBudget,
}

// A reservation is matched at once; boxing Publish would allocate on every publication.
#[allow(clippy::large_enum_variant)]
pub(crate) enum Reserved<K, T> {
    Reuse(T),
    Publish {
        result: T,
        recipe: Direct<K>,
        bytes: Box<[u8]>,
        bundle: Option<bundle::Pending>,
    },
}

impl Store {
    /// Enter mutation ownership before acquiring any execution/project lease.
    pub(crate) fn with_ownership<T>(
        &self,
        evaluate: impl FnOnce(&Ownership<'_>) -> Result<T, Error>,
    ) -> Result<T, Error> {
        lease::with_ownership(&self.root, self.project, evaluate)
    }

    /// Healthy retained hits may only acquire previously admitted controls.
    pub(crate) fn with_existing_ownership<T>(
        &self,
        evaluate: impl FnOnce(&Ownership<'_>) -> Result<T, Error>,
    ) -> Result<T, Error> {
        lease::with_ownership(&self.root, self.project, evaluate)
    }
    /// Open an existing trusted root and initialize its fixed v1 layout.
    #[cfg(test)]
    pub(crate) fn open(root: &Path, project: [u8; 32]) -> Result<Self, Error> {
        Self::open_with_budget(root, project, crate::node::GraphBudget::new())
    }

    fn open_with_budget(
        root: &Path,
        project: [u8; 32],
        graph_budget: crate::node::GraphBudget,
    ) -> Result<Self, Error> {
        let metadata = fs::symlink_metadata(root).map_err(|source| Error::Io {
            operation: "inspect typed store root",
            path: root.to_path_buf(),
            source,
        })?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(Error::Shape(root.to_path_buf()));
        }
        let root = fs::canonicalize(root).map_err(|source| Error::Io {
            operation: "canonicalize typed store root",
            path: root.to_path_buf(),
            source,
        })?;
        // A store another format wrote is a clean miss: it is removed and
        // rebuilt, never migrated.
        let fresh = match accounting::current_format(&root)? {
            Some(true) => false,
            Some(false) => {
                clear(&root)?;
                true
            }
            None => true,
        };
        let mut budget = recovery::Budget::new();
        lease::with_bootstrap(&root, fresh, || {
            for directory in FIXED_DIRECTORIES {
                if fresh {
                    entry::ensure_directory(&root, Path::new(directory))?;
                } else if entry::existing_directory(&root, Path::new(directory))?.is_none() {
                    return Err(Error::CleanupRequired(
                        "typed store fixed directory disappeared after bootstrap",
                    ));
                }
            }
            lease::initialize(&root, fresh)?;
            if fresh {
                lease::with_publication(&root, |publication| {
                    accounting::initialize(publication, &root, &mut budget)
                })?;
            }
            Ok(())
        })?;
        lease::enroll_project(&root, Digest::from_bytes(project), &mut budget)?;
        Ok(Self {
            root,
            project: Digest::from_bytes(project),
            graph_budget,
        })
    }

    /// Open the project's store, `<target>/rawr/store`.
    pub(crate) fn open_project(
        rawr: &Path,
        project: [u8; 32],
        graph_budget: crate::node::GraphBudget,
    ) -> Result<Self, Error> {
        let root = entry::ensure_project_root(rawr)?;
        Self::open_with_budget(&root, project, graph_budget)
    }

    #[cfg(test)]
    pub(crate) fn evict_all(&self) -> Result<u64, Error> {
        let before = accounting::usage(&self.root)?.0.bytes;
        while eviction::recover_one(
            &self.root,
            self.project,
            false,
            false,
            &mut recovery::Budget::new(),
        )? {}
        Ok(before.saturating_sub(accounting::usage(&self.root)?.0.bytes))
    }

    /// Borrow the canonical store root.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub(crate) const fn graph_budget(&self) -> &crate::node::GraphBudget {
        &self.graph_budget
    }

    /// Retire one isolated test-fixture store, including immutable bundles.
    #[cfg(any(test, feature = "test-fixtures"))]
    #[doc(hidden)]
    pub fn retire_fixture(self) -> Result<(), Error> {
        bundle::retire_fixture_store(&self.root)
    }

    #[cfg(test)]
    pub(crate) fn fixture_usage(&self) -> Result<(u64, u64, u64, usize), Error> {
        let (charge, reservations) = accounting::usage(&self.root)?;
        Ok((
            charge.bytes,
            charge.allocations,
            charge.members,
            reservations,
        ))
    }

    #[cfg(test)]
    pub(crate) fn fixture_physical_inventory(&self) -> Result<(), Error> {
        accounting::physical_usage(&self.root).map(|_| ())
    }

    /// The installation ledger total after a complete physical inventory.
    #[cfg(test)]
    pub(crate) fn fixture_ledger_total(&self) -> Result<(u64, u64, u64), Error> {
        accounting::physical_usage(&self.root)
            .map(|(total, _)| (total.bytes, total.allocations, total.members))
    }

    #[cfg(test)]
    pub(crate) fn fixture_project_usage(&self) -> Result<(u64, u64, u64, usize, usize), Error> {
        let (charge, reservations, roots) = pins::usage(&self.root, self.project)?;
        Ok((
            charge.bytes,
            charge.allocations,
            charge.members,
            reservations,
            roots,
        ))
    }

    #[cfg(test)]
    pub(crate) fn fixture_pin_counts(&self) -> Result<(usize, usize), Error> {
        pins::lock::fixture_counts(&self.root)
    }

    #[cfg(test)]
    pub(crate) fn fixture_evict_one(&self, local_only: bool) -> Result<bool, Error> {
        eviction::recover_one(
            &self.root,
            self.project,
            local_only,
            false,
            &mut recovery::Budget::new(),
        )
    }

    #[cfg(test)]
    pub(crate) fn fixture_retire_excess(&self, kind: Kind, keep: usize) -> Result<usize, Error> {
        let mut retired = 0;
        while eviction::recover_resolution_excess(
            &self.root,
            self.project,
            kind,
            keep,
            &mut recovery::Budget::new(),
        )? {
            retired += 1;
        }
        Ok(retired)
    }

    #[cfg(test)]
    /// Make room for a write as large as the whole installation target.
    pub(crate) fn fixture_make_room(&self) -> Result<(), Error> {
        self.make_room(accounting::Charge {
            bytes: u64::MAX / 2,
            allocations: 0,
            members: 0,
        })
    }

    #[cfg(test)]
    pub(crate) fn fixture_interrupt_oldest_eviction(&self) -> Result<(), Error> {
        eviction::interrupt_oldest(&self.root, self.project)
    }

    #[cfg(test)]
    pub(crate) fn fixture_has_pending_eviction(&self) -> Result<bool, Error> {
        accounting::pending_eviction_recovery(&self.root, &mut recovery::Budget::new())
            .map(|pending| !pending.is_empty())
    }

    #[cfg(test)]
    pub(crate) fn fixture_interrupt_after_staging_action(&self) -> Result<PathBuf, Error> {
        eviction::interrupt_after_staging_action(&self.root, self.project)
    }

    #[cfg(test)]
    pub(crate) fn fixture_interrupt_after_thawing_bundle_root(&self) -> Result<PathBuf, Error> {
        eviction::interrupt_after_thawing_bundle_root(&self.root, self.project)
    }

    #[cfg(test)]
    pub(crate) fn fixture_interrupt_planned_action<K: Value>(
        &self,
        recipe: &Direct<K>,
    ) -> Result<PathBuf, Error> {
        lease::with_publication(&self.root, |publication| {
            entry::stage_retirement(
                publication,
                &self.root,
                &self.action_path::<K>(recipe.action()),
            )
        })
    }

    #[cfg(test)]
    pub(crate) fn fixture_project_ledger_exists(&self) -> bool {
        self.root
            .join("ledgers/projects")
            .join(self.project.directory())
            .exists()
    }

    #[cfg(test)]
    pub(crate) fn fixture_stage_uncommitted<K: Value>(
        &self,
        recipe: &Direct<K>,
        bytes: &[u8],
    ) -> Result<(), Error> {
        let estimate = accounting::Charge::node(
            recipe.output_limit(),
            Envelope::reservation_length(recipe.parent_count())?,
        )?;
        self.with_action_lease(recipe.action(), || {
            lease::with_project(&self.root, self.project, || {
                let key = accounting::LeaseKey::Action(recipe.action());
                let mut recovery = recovery::Budget::new();
                let (_project, reservation) =
                    self.reserve_node(key, estimate, K::KIND, K::SCHEMA, &mut recovery)?;
                lease::with_publication(&self.root, |_publication| {
                    self.publish_reserved(_publication, recipe, bytes, &reservation, None)
                        .map(|_| ())
                })
            })
        })
    }

    pub(crate) fn lookup<K: Value>(
        &self,
        recipe: &Direct<K>,
        budget: &crate::node::GraphBudget,
    ) -> Result<Option<crate::node::Validated<K>>, Error> {
        self.lookup_with(recipe, budget, |stored| stored.decode(recipe, budget))
    }

    pub(crate) fn lookup_staged<K: Value>(
        &self,
        recipe: &Direct<K>,
        budget: &crate::node::GraphBudget,
    ) -> Result<Option<crate::node::replay::Staged<K>>, Error> {
        self.lookup_with(recipe, budget, |stored| stored.stage(recipe, budget))
    }

    fn lookup_with<K: Value, T>(
        &self,
        recipe: &Direct<K>,
        budget: &crate::node::GraphBudget,
        decode: impl FnOnce(Stored<K>) -> Result<T, Error>,
    ) -> Result<Option<T>, Error> {
        let action_relative = self.action_relative::<K>(recipe.action());
        let Some(action) =
            entry::existing_directory_budgeted(&self.root, &action_relative, budget)?
        else {
            return Ok(None);
        };

        let envelope_path = action.join("envelope");
        let envelope_bytes = entry::read_file_budgeted(&envelope_path, MAX_ENVELOPE_BYTES, budget)?;
        self.validate_allocation(
            &action,
            accounting::Class::Action,
            K::KIND,
            K::SCHEMA,
            Digest::from_bytes(recipe.action().as_bytes()),
            &envelope_bytes,
            "envelope",
            Some(budget),
        )?;
        let envelope = Envelope::decode(&envelope_bytes).map_err(|_| Error::Corrupt {
            path: envelope_path,
            reason: "envelope decoding failed",
        })?;
        if !envelope.matches(recipe) {
            return Err(Error::Corrupt {
                path: action,
                reason: "envelope does not match its action path and current recipe",
            });
        }
        let content_relative = self.content_relative::<K>(envelope.content);
        let content = entry::existing_directory_budgeted(&self.root, &content_relative, budget)?
            .ok_or_else(|| Error::Corrupt {
                path: self.root.join(&content_relative),
                reason: "content-addressed directory is missing",
            })?;
        let content_path = content.join("payload");
        let before = fs::symlink_metadata(&content_path).ok();
        let bytes = entry::read_payload_budgeted(
            &content_path,
            envelope.content_length,
            envelope.output_limit,
            budget,
        )?;
        let (sha, current) = payload_digests::<K>(&content_path, before.as_ref(), &bytes);
        let record = self.validate_allocation_record(
            &content,
            accounting::Class::Content,
            K::KIND,
            K::SCHEMA,
            envelope.content,
            (bytes.len(), sha),
            "payload",
            Some(budget),
            accounting::Record::read_budgeted,
        )?;
        accounting::registered_with_budget(&self.root, &record, Some(budget))?;
        budget.hash(bytes.len())?;
        if bytes.len() != envelope.content_length || current != envelope.content {
            return Err(Error::Corrupt {
                path: content_path,
                reason: "content length or digest changed",
            });
        }
        let bundled = accounting::action_is_bundled_with_budget(
            &self.root,
            K::KIND,
            K::SCHEMA,
            recipe.action(),
            envelope.content,
            Some(budget),
        )?;
        decode(Stored::new(envelope, bytes, bundled)).map(Some)
    }

    fn publish_reserved<K: Value>(
        &self,
        publication: &lease::Publication<'_>,
        recipe: &Direct<K>,
        bytes: &[u8],
        reservation: &accounting::Reservation,
        mut bundle: Option<&mut bundle::Pending>,
    ) -> Result<Box<[accounting::Record]>, Error> {
        let digest = content_digest::<K>(bytes);
        let envelope = Envelope::direct(recipe, digest, bytes.len())?.encode()?;
        accounting::plan(
            publication,
            &self.root,
            reservation,
            K::KIND,
            K::SCHEMA,
            recipe.action(),
            digest,
            bundle.is_some(),
        )?;
        let mut records = Vec::new();
        records
            .try_reserve_exact(2 + usize::from(bundle.is_some()))
            .map_err(|_| Error::Allocation)?;
        if let Some(bundle) = bundle.as_mut() {
            records.push(bundle.publish_observed(
                K::KIND,
                K::SCHEMA,
                digest,
                reservation.key(),
                self.project,
            )?);
        }
        let content_parent = self.content_parent::<K>();
        entry::ensure_admitted_directory(publication, &self.root, &content_parent)?;
        let content_target = self.content_path::<K>(digest);
        let content_identity = accounting::Identity {
            class: accounting::Class::Content,
            kind: K::KIND,
            schema: K::SCHEMA,
            key: digest,
            reservation: reservation.key(),
            publisher: self.project,
        };
        let (content_publication, published_content) = entry::publish_sealed_directory(
            &self.root,
            &content_target,
            "payload",
            bytes,
            |metadata| {
                let (encoded, record) =
                    accounting::Record::seal(content_identity, bytes, metadata)?;
                Ok((encoded, record))
            },
        )?;
        match content_publication {
            entry::Publication::Published => {
                records.push(published_content.ok_or(Error::State(
                    "published content omitted its allocation charge",
                ))?);
            }
            entry::Publication::Existing => {
                let existing = entry::read_file(&content_target.join("payload"), bytes.len())?;
                if existing.as_ref() != bytes {
                    return Err(Error::Corrupt {
                        path: content_target,
                        reason: "equal content digest resolved to unequal bytes",
                    });
                }
                self.validate_allocation(
                    &content_target,
                    accounting::Class::Content,
                    K::KIND,
                    K::SCHEMA,
                    digest,
                    &existing,
                    "payload",
                    None,
                )?;
            }
        }

        let action_parent = PathBuf::from("actions")
            .join(K::KIND.directory())
            .join(crate::node::STORAGE_DIRECTORY);
        entry::ensure_admitted_directory(publication, &self.root, &action_parent)?;
        let action_target = self.action_path::<K>(recipe.action());
        let action_identity = accounting::Identity {
            class: accounting::Class::Action,
            kind: K::KIND,
            schema: K::SCHEMA,
            key: Digest::from_bytes(recipe.action().as_bytes()),
            reservation: reservation.key(),
            publisher: self.project,
        };
        let (publication, published_action) = entry::publish_sealed_directory(
            &self.root,
            &action_target,
            "envelope",
            &envelope,
            |metadata| {
                let (encoded, record) =
                    accounting::Record::seal(action_identity, &envelope, metadata)?;
                Ok((encoded, record))
            },
        )?;
        match publication {
            entry::Publication::Published => {
                records.push(published_action.ok_or(Error::State(
                    "published action omitted its allocation charge",
                ))?);
                Ok(records.into_boxed_slice())
            }
            entry::Publication::Existing => Err(Error::Corrupt {
                path: action_target,
                reason: "action appeared after its authoritative miss recheck",
            }),
        }
    }

    pub(crate) fn with_action_reservation<K: Value, T>(
        &self,
        ownership: &Ownership<'_>,
        recipe: &Direct<K>,
        in_flight: Option<&crate::node::ReadSetProof>,
        evaluate: impl FnOnce() -> Result<Reserved<K, T>, Error>,
    ) -> Result<T, Error> {
        self.with_node_reservation(
            ownership,
            accounting::LeaseKey::Action(recipe.action()),
            recipe.output_limit(),
            recipe.parent_count(),
            in_flight.map(InFlight::Current),
            evaluate,
        )
    }

    pub(crate) fn with_staged_action_reservation<K: Value, T, U>(
        &self,
        ownership: &Ownership<'_>,
        recipe: &Direct<K>,
        retention: &crate::node::read_set::Retention,
        evaluate: impl FnOnce() -> Result<Reserved<K, T>, Error>,
        after_publish: impl FnOnce(T) -> Result<U, Error>,
    ) -> Result<U, Error> {
        ownership.require(&self.root, self.project)?;
        retention.require_parents(recipe.parents())?;
        lease::with_project(&self.root, self.project, || {
            self.with_node_reservation_locked(
                ownership,
                accounting::LeaseKey::Action(recipe.action()),
                recipe.output_limit(),
                recipe.parent_count(),
                Some(InFlight::Retained(retention)),
                evaluate,
                after_publish,
            )
        })
    }

    pub(crate) fn with_staged_batch<T>(
        &self,
        initial: &crate::node::read_set::Retention,
        sink: &dyn crate::metrics::Sink,
        operation: impl FnOnce(&mut Batch<'_>) -> Result<T, Error>,
    ) -> Result<T, Error> {
        self.with_existing_ownership(|ownership| {
            let mut batch = Batch::begin(self, initial, sink, ownership)?;
            let result = operation(&mut batch);
            batch.finish(result)
        })
    }

    // Caller owns the project mutation lease. The returned OS pin may outlive
    // that lease, allowing the next pure stage to acquire its action lease.
    fn retain_locked(
        &self,
        ownership: &Ownership<'_>,
        retention: &crate::node::read_set::Retention,
    ) -> Result<PinGuard, Error> {
        ownership.require(&self.root, self.project)?;
        retention.validate_storage(self)?;
        let admission = lease::with_publication(&self.root, |publication| {
            transaction::Transaction::bind_retained(publication, self, retention)?.finish(
                || retention.validate_storage(self),
                || Ok::<(), std::convert::Infallible>(()),
            )
        })?;
        match admission {
            Ok(((), [guard])) => Ok(guard),
            Err(
                transaction::Rejected::Operation(never)
                | transaction::Rejected::Cleanup { failure: never, .. },
            ) => match never {},
        }
    }

    /// Keep historical parent allocations pinned throughout a hit's promotion.
    pub(crate) fn with_retained_in_flight<T>(
        &self,
        ownership: &Ownership<'_>,
        retention: &crate::node::read_set::Retention,
        operation: impl FnOnce() -> Result<T, Error>,
    ) -> Result<T, Error> {
        ownership.require(&self.root, self.project)?;
        lease::with_project(&self.root, self.project, || {
            retention.validate_storage(self)?;
            let admission = lease::with_publication(&self.root, |publication| {
                transaction::Transaction::bind_retained(publication, self, retention)?.finish(
                    || retention.validate_storage(self),
                    || Ok::<(), std::convert::Infallible>(()),
                )
            })?;
            let guard = match admission {
                Ok(((), [guard])) => guard,
                Err(
                    transaction::Rejected::Operation(never)
                    | transaction::Rejected::Cleanup { failure: never, .. },
                ) => match never {},
            };
            let result = operation();
            self.finish_in_flight(
                ownership,
                Some(InFlight::Retained(retention)),
                Some(guard),
                result,
            )
        })
    }

    pub(crate) fn with_execution_reservation<K: Value, T>(
        &self,
        ownership: &Ownership<'_>,
        recipe: &crate::node::Discoverable<K>,
        in_flight: Option<&crate::node::ReadSetProof>,
        evaluate: impl FnOnce() -> Result<Reserved<K, T>, Error>,
    ) -> Result<T, Error> {
        self.with_node_reservation(
            ownership,
            accounting::LeaseKey::Execution(recipe.execution()),
            recipe.output_limit(),
            recipe.parent_count(),
            in_flight.map(InFlight::Current),
            evaluate,
        )
    }

    #[cfg(test)]
    pub(crate) fn with_execution_bundle_reservation<K: Value, T>(
        &self,
        ownership: &Ownership<'_>,
        recipe: &crate::node::Discoverable<K>,
        in_flight: Option<&crate::node::ReadSetProof>,
        evaluate: impl FnOnce(bundle::Attempt) -> Result<Reserved<K, T>, Error>,
    ) -> Result<T, Error> {
        self.with_rejected_execution_bundle_reservation(ownership, recipe, in_flight, &[], evaluate)
    }

    pub(crate) fn with_rejected_execution_bundle_reservation<K: Value, T>(
        &self,
        ownership: &Ownership<'_>,
        recipe: &crate::node::Discoverable<K>,
        in_flight: Option<&crate::node::ReadSetProof>,
        rejected_actions: &[Option<(ActionKey, crate::node::Corruption)>],
        evaluate: impl FnOnce(bundle::Attempt) -> Result<Reserved<K, T>, Error>,
    ) -> Result<T, Error> {
        self.with_bundle_reservation(
            ownership,
            recipe,
            in_flight.map(InFlight::Current),
            rejected_actions,
            evaluate,
        )
    }

    pub(crate) fn with_retained_execution_bundle_reservation<K: Value, T>(
        &self,
        ownership: &Ownership<'_>,
        recipe: &crate::node::Discoverable<K>,
        retention: &crate::node::read_set::Retention,
        rejected_actions: &[Option<(ActionKey, crate::node::Corruption)>],
        evaluate: impl FnOnce(bundle::Attempt) -> Result<Reserved<K, T>, Error>,
    ) -> Result<T, Error> {
        retention.covers_parents(recipe.parents())?;
        self.with_bundle_reservation(
            ownership,
            recipe,
            Some(InFlight::Retained(retention)),
            rejected_actions,
            evaluate,
        )
    }

    fn with_bundle_reservation<K: Value, T>(
        &self,
        ownership: &Ownership<'_>,
        recipe: &crate::node::Discoverable<K>,
        in_flight: Option<InFlight<'_>>,
        rejected_actions: &[Option<(ActionKey, crate::node::Corruption)>],
        evaluate: impl FnOnce(bundle::Attempt) -> Result<Reserved<K, T>, Error>,
    ) -> Result<T, Error> {
        let parents = match in_flight {
            Some(InFlight::Current(proof)) => proof.entries(),
            Some(InFlight::Retained(retention)) => retention.entries(),
            None => &[],
        };
        ownership.require(&self.root, self.project)?;
        let key = accounting::LeaseKey::Execution(recipe.execution());
        lease::with_project(&self.root, self.project, || {
            let mut recovery = recovery::Budget::new();
            eviction::recover_pending(&self.root, &mut recovery)?;
            eviction::recover_rejected(
                &self.root,
                K::KIND,
                K::SCHEMA,
                rejected_actions,
                parents,
                &mut recovery,
            )?;
            let in_flight_guard = self.acquire_in_flight(ownership, in_flight, &mut recovery)?;
            let result = (|| {
                let envelope = Envelope::reservation_length(recipe.parent_count())?;
                let node = accounting::Charge::node(recipe.output_limit(), envelope)?;
                let (project, reservation, budget) =
                    self.reserve_bundle_node(key, node, K::KIND, K::SCHEMA, &mut recovery)?;
                let attempt =
                    match bundle::Attempt::allocate(&self.root, recipe.execution(), budget) {
                        Ok(attempt) => attempt,
                        Err(failure) => {
                            return match lease::with_publication(&self.root, |publication| {
                                self.cancel_node(publication, &project, &reservation)
                            }) {
                                Ok(()) => Err(failure),
                                Err(cleanup) => Err(Error::Cleanup {
                                    failure: Box::new(failure),
                                    cleanup: Box::new(cleanup),
                                }),
                            };
                        }
                    };
                match evaluate(attempt) {
                    Ok(Reserved::Reuse(result)) => {
                        lease::with_publication(&self.root, |publication| {
                            self.cancel_node(publication, &project, &reservation)
                        })?;
                        Ok(result)
                    }
                    Ok(Reserved::Publish {
                        result,
                        recipe,
                        bytes,
                        mut bundle,
                    }) => {
                        let Some(mut bundle) = bundle.take() else {
                            return Err(Error::State(
                                "bundled node publication omitted its execution bundle",
                            ));
                        };
                        lease::with_publication(&self.root, |publication| {
                            let records = self.publish_reserved(
                                publication,
                                &recipe,
                                &bytes,
                                &reservation,
                                Some(&mut bundle),
                            )?;
                            accounting::commit(publication, &self.root, &reservation, &records)?;
                            bundle.retain();
                            pins::cancel(publication, &self.root, self.project, &project)?;
                            Ok(())
                        })?;
                        Ok(result)
                    }
                    Err(failure) => match lease::with_publication(&self.root, |publication| {
                        self.cancel_node(publication, &project, &reservation)
                    }) {
                        Ok(()) => Err(failure),
                        Err(cleanup) => Err(Error::Cleanup {
                            failure: Box::new(failure),
                            cleanup: Box::new(cleanup),
                        }),
                    },
                }
            })();
            self.finish_in_flight(ownership, in_flight, in_flight_guard, result)
        })
    }

    fn with_node_reservation<K: Value, T>(
        &self,
        ownership: &Ownership<'_>,
        key: accounting::LeaseKey,
        output_limit: usize,
        parent_count: usize,
        in_flight: Option<InFlight<'_>>,
        evaluate: impl FnOnce() -> Result<Reserved<K, T>, Error>,
    ) -> Result<T, Error> {
        ownership.require(&self.root, self.project)?;
        lease::with_project(&self.root, self.project, || {
            self.with_node_reservation_locked(
                ownership,
                key,
                output_limit,
                parent_count,
                in_flight,
                evaluate,
                Ok,
            )
        })
    }

    #[allow(clippy::too_many_arguments)]
    fn with_node_reservation_locked<K: Value, T, U>(
        &self,
        ownership: &Ownership<'_>,
        key: accounting::LeaseKey,
        output_limit: usize,
        parent_count: usize,
        in_flight: Option<InFlight<'_>>,
        evaluate: impl FnOnce() -> Result<Reserved<K, T>, Error>,
        after_publish: impl FnOnce(T) -> Result<U, Error>,
    ) -> Result<U, Error> {
        let mut recovery = recovery::Budget::new();
        let in_flight_guard = self.acquire_in_flight(ownership, in_flight, &mut recovery)?;
        let result = (|| {
            let envelope = Envelope::reservation_length(parent_count)?;
            let estimate = accounting::Charge::node(output_limit, envelope)?;
            let (project, reservation) =
                self.reserve_node(key, estimate, K::KIND, K::SCHEMA, &mut recovery)?;
            match evaluate() {
                Ok(Reserved::Reuse(result)) => {
                    lease::with_publication(&self.root, |publication| {
                        self.cancel_node(publication, &project, &reservation)
                    })?;
                    Ok(result)
                }
                Ok(Reserved::Publish {
                    result,
                    recipe,
                    bytes,
                    mut bundle,
                }) => {
                    lease::with_publication(&self.root, |publication| {
                        let records = self.publish_reserved(
                            publication,
                            &recipe,
                            &bytes,
                            &reservation,
                            bundle.as_mut(),
                        )?;
                        accounting::commit(publication, &self.root, &reservation, &records)?;
                        if let Some(bundle) = bundle.take() {
                            bundle.retain();
                        }
                        pins::cancel(publication, &self.root, self.project, &project)?;
                        Ok(())
                    })?;
                    Ok(result)
                }
                Err(failure) => {
                    match lease::with_publication(&self.root, |publication| {
                        self.cancel_node(publication, &project, &reservation)
                    }) {
                        Ok(()) => Err(failure),
                        Err(cleanup) => Err(Error::Cleanup {
                            failure: Box::new(failure),
                            cleanup: Box::new(cleanup),
                        }),
                    }
                }
            }
        })();
        let result = result.and_then(after_publish);
        self.finish_in_flight(ownership, in_flight, in_flight_guard, result)
    }

    fn reserve_node(
        &self,
        key: accounting::LeaseKey,
        estimate: accounting::Charge,
        kind: Kind,
        schema: u32,
        recovery: &mut recovery::Budget,
    ) -> Result<(pins::Reservation, accounting::Reservation), Error> {
        eviction::recover_pending(&self.root, recovery)?;
        self.recover_abandoned_reservation(key, recovery)?;
        bundle::recover_dead(&self.root, recovery)?;
        self.make_room(estimate)?;
        lease::with_publication(&self.root, |publication| {
            self.reserve_node_once(publication, key, estimate, kind, schema, recovery)
        })
    }

    /// Store and project sizes are eviction targets, never refusals: evict
    /// released closures, oldest first, while a target is exceeded and
    /// something is evictable; each project's Live and LastGood runners stay.
    /// Each eviction is its own bounded recovery pass.
    fn make_room(&self, estimate: accounting::Charge) -> Result<(), Error> {
        // A project keeps a few unrooted resolutions, whatever the caps.
        while eviction::recover_resolution_excess(
            &self.root,
            self.project,
            Kind::Resolution,
            eviction::RESOLUTIONS_KEPT,
            &mut recovery::Budget::new(),
        )? {}
        loop {
            let installation = accounting::over_cap(&self.root, estimate)?;
            if !(installation || pins::over_cap(&self.root, self.project, estimate)?) {
                return Ok(());
            }
            if !eviction::recover_one(
                &self.root,
                self.project,
                !installation,
                true,
                &mut recovery::Budget::new(),
            )? {
                return Ok(());
            }
        }
    }

    fn reserve_node_once(
        &self,
        publication: &lease::Publication<'_>,
        key: accounting::LeaseKey,
        estimate: accounting::Charge,
        kind: Kind,
        schema: u32,
        recovery: &mut recovery::Budget,
    ) -> Result<(pins::Reservation, accounting::Reservation), Error> {
        recovery::recover_temporaries(publication, &self.root, recovery)?;
        accounting::recover_key(publication, &self.root, key, recovery)?;
        pins::recover(publication, &self.root, self.project, key, recovery)?;
        pins::recover_empty_before_new_project(publication, &self.root, self.project, recovery)?;
        self.prepare_node_parents(publication, kind, schema)?;
        self.reserve_recovered(publication, key, estimate)
    }

    fn reserve_recovered(
        &self,
        publication: &lease::Publication<'_>,
        key: accounting::LeaseKey,
        estimate: accounting::Charge,
    ) -> Result<(pins::Reservation, accounting::Reservation), Error> {
        let installation = accounting::reserve(publication, &self.root, key, estimate)?;
        match pins::reserve(publication, &self.root, self.project, key, estimate) {
            Ok(project) => Ok((project, installation)),
            Err(failure) => match accounting::cancel(publication, &self.root, &installation) {
                Ok(()) => Err(failure),
                Err(cleanup) => Err(Error::Cleanup {
                    failure: Box::new(failure),
                    cleanup: Box::new(cleanup),
                }),
            },
        }
    }

    fn reserve_bundle_node(
        &self,
        key: accounting::LeaseKey,
        node: accounting::Charge,
        kind: Kind,
        schema: u32,
        recovery: &mut recovery::Budget,
    ) -> Result<(pins::Reservation, accounting::Reservation, bundle::Budget), Error> {
        eviction::recover_pending(&self.root, recovery)?;
        self.recover_abandoned_reservation(key, recovery)?;
        bundle::recover_dead(&self.root, recovery)?;
        self.make_room(node)?;
        lease::with_publication(&self.root, |publication| {
            self.reserve_bundle_node_once(publication, key, node, kind, schema, recovery)
        })
    }

    fn reserve_bundle_node_once(
        &self,
        publication: &lease::Publication<'_>,
        key: accounting::LeaseKey,
        node: accounting::Charge,
        kind: Kind,
        schema: u32,
        recovery: &mut recovery::Budget,
    ) -> Result<(pins::Reservation, accounting::Reservation, bundle::Budget), Error> {
        recovery::recover_temporaries(publication, &self.root, recovery)?;
        accounting::recover_key(publication, &self.root, key, recovery)?;
        pins::recover(publication, &self.root, self.project, key, recovery)?;
        pins::recover_empty_before_new_project(publication, &self.root, self.project, recovery)?;
        let accounting::LeaseKey::Execution(execution) = key else {
            return Err(Error::State(
                "runner bundle reservation requires one execution lease",
            ));
        };
        bundle::recover_orphans(&self.root, execution, recovery)?;
        bundle::prepare_execution(publication, &self.root, execution)?;
        self.prepare_node_parents(publication, kind, schema)?;
        self.reserve_bundle_recovered(publication, key, node)
    }

    fn prepare_node_parents(
        &self,
        publication: &lease::Publication<'_>,
        kind: Kind,
        schema: u32,
    ) -> Result<(), Error> {
        entry::ensure_admitted_directory(
            publication,
            &self.root,
            &PathBuf::from("actions")
                .join(kind.directory())
                .join(crate::node::STORAGE_DIRECTORY),
        )?;
        entry::ensure_admitted_directory(
            publication,
            &self.root,
            &PathBuf::from("content")
                .join(kind.directory())
                .join(format!("{schema:08x}")),
        )?;
        Ok(())
    }

    fn recover_abandoned_reservation(
        &self,
        current: accounting::LeaseKey,
        budget: &mut recovery::Budget,
    ) -> Result<(), Error> {
        let keys = accounting::reservation_keys_header_recovery(&self.root)?;
        for key in keys.iter().copied().filter(|key| *key != current) {
            let Some(_guard) = lease::try_key(&self.root, key)? else {
                continue;
            };
            lease::with_publication(&self.root, |publication| {
                recovery::recover_temporaries(publication, &self.root, budget)?;
                pins::recover_reservation_only(publication, &self.root, key)?;
                accounting::recover_key(publication, &self.root, key, budget)?;
                Ok(())
            })?;
            return Ok(());
        }
        Ok(())
    }

    fn reserve_bundle_recovered(
        &self,
        publication: &lease::Publication<'_>,
        key: accounting::LeaseKey,
        node: accounting::Charge,
    ) -> Result<(pins::Reservation, accounting::Reservation, bundle::Budget), Error> {
        let project = pins::available(&self.root, self.project, key)?;
        let (installation, project_admitted) =
            accounting::available(publication, &self.root, project.plan())?;
        let (estimate, budget) = bundle::Budget::bounded(node)?;
        let reservation = installation.reserve(key, estimate)?;
        match project.reserve(project_admitted, publication, &self.root, estimate) {
            Ok(project) => Ok((project, reservation, budget)),
            Err(failure) => match accounting::cancel(publication, &self.root, &reservation) {
                Ok(()) => Err(failure),
                Err(cleanup) => Err(Error::Cleanup {
                    failure: Box::new(failure),
                    cleanup: Box::new(cleanup),
                }),
            },
        }
    }

    fn cancel_node(
        &self,
        publication: &lease::Publication<'_>,
        project: &pins::Reservation,
        installation: &accounting::Reservation,
    ) -> Result<(), Error> {
        accounting::cancel(publication, &self.root, installation)?;
        pins::cancel(publication, &self.root, self.project, project)
    }

    fn finish_in_flight<T>(
        &self,
        ownership: &Ownership<'_>,
        proof: Option<InFlight<'_>>,
        guard: Option<PinGuard>,
        result: Result<T, Error>,
    ) -> Result<T, Error> {
        ownership.require(&self.root, self.project)?;
        drop(guard);
        let cleanup = proof
            .map(|proof| {
                lease::with_publication(&self.root, |publication| {
                    pins::release_in_flight(publication, &self.root, self.project, proof.digest())
                })
            })
            .transpose()
            .map(|_| ());
        match (result, cleanup) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(failure), Ok(())) => Err(failure),
            (Ok(_), Err(cleanup)) => Err(cleanup),
            (Err(failure), Err(cleanup)) => Err(Error::Cleanup {
                failure: Box::new(failure),
                cleanup: Box::new(cleanup),
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn with_pin<T>(
        &self,
        role: PinRole,
        proof: &crate::node::ReadSetProof,
        evaluate: impl FnOnce() -> Result<T, Error>,
    ) -> Result<T, Error> {
        if role == PinRole::InFlight {
            return self.with_existing_ownership(|ownership| {
                lease::with_project(&self.root, self.project, || {
                    let guard = Some(self.acquire_pin_project_locked(role, proof)?);
                    let result = evaluate();
                    self.finish_in_flight(ownership, Some(InFlight::Current(proof)), guard, result)
                })
            });
        }
        let _guard = self.acquire_pin(role, proof)?;
        evaluate()
    }

    pub(crate) fn acquire_pin(
        &self,
        role: PinRole,
        proof: &crate::node::ReadSetProof,
    ) -> Result<pins::Guard, Error> {
        if role == PinRole::InFlight {
            return Err(Error::State(
                "InFlight pin requires scoped project ownership",
            ));
        }
        lease::with_project(&self.root, self.project, || {
            self.acquire_pin_project_locked(role, proof)
        })
    }

    pub(crate) fn acquire_prepared_retained(
        &self,
        proof: &crate::node::ReadSetProof,
        retention: &crate::node::read_set::Retention,
    ) -> Result<pins::Guard, Error> {
        match self.with_retained_pin_roles(
            [PinRole::Prepared],
            proof,
            retention,
            crate::node::read_set::Scope::Consumer,
            || Ok::<(), std::convert::Infallible>(()),
        )? {
            Ok(((), [guard])) => Ok(guard),
            Err(
                transaction::Rejected::Operation(never)
                | transaction::Rejected::Cleanup { failure: never, .. },
            ) => match never {},
        }
    }

    /// Storage ownership for an explicit startup handoff. This does not admit a
    /// consumer: bootstrap must still validate immediately before spawning.
    pub(crate) fn acquire_startup_retained(
        &self,
        proof: &crate::node::ReadSetProof,
        retention: &crate::node::read_set::Retention,
    ) -> Result<pins::Guard, Error> {
        retention.require_proof(proof)?;
        let result = lease::with_project(&self.root, self.project, || {
            retention.validate_storage(self)?;
            lease::with_publication(&self.root, |publication| {
                let mut recovery = recovery::Budget::new();
                pins::recover_empty_before_new_project(
                    publication,
                    &self.root,
                    self.project,
                    &mut recovery,
                )?;
                transaction::Transaction::bind(
                    publication,
                    &self.root,
                    self.project,
                    [PinRole::Prepared],
                    proof,
                )?
                .finish(
                    || retention.validate_storage(self),
                    || Ok::<(), std::convert::Infallible>(()),
                )
            })
        })?;
        match result {
            Ok(((), [guard])) => Ok(guard),
            Err(
                transaction::Rejected::Operation(never)
                | transaction::Rejected::Cleanup { failure: never, .. },
            ) => match never {},
        }
    }

    pub(crate) fn with_live_last_good_retained<T, E>(
        &self,
        proof: &crate::node::ReadSetProof,
        retention: &crate::node::read_set::Retention,
        operation: impl FnOnce() -> Result<T, E>,
    ) -> Result<transaction::Admitted<T, E, 2>, Error> {
        self.with_retained_pin_roles(
            [PinRole::Live, PinRole::LastGood],
            proof,
            retention,
            crate::node::read_set::Scope::Complete,
            operation,
        )
    }

    fn with_retained_pin_roles<T, E, const N: usize>(
        &self,
        roles: [PinRole; N],
        proof: &crate::node::ReadSetProof,
        retention: &crate::node::read_set::Retention,
        scope: crate::node::read_set::Scope,
        operation: impl FnOnce() -> Result<T, E>,
    ) -> Result<transaction::Admitted<T, E, N>, Error> {
        retention.require_proof(proof)?;
        lease::with_project(&self.root, self.project, || {
            retention.validate_storage(self)?;
            lease::with_publication(&self.root, |publication| {
                let mut recovery = recovery::Budget::new();
                // The unchanged transaction keeps predecessors through its
                // post-bind CURRENT proof check and the admission callback.
                self.with_pin_roles_at(publication, roles, proof, &mut recovery, scope, operation)
            })
        })
    }

    fn acquire_pin_project_locked(
        &self,
        role: PinRole,
        proof: &crate::node::ReadSetProof,
    ) -> Result<pins::Guard, Error> {
        match self.with_pin_roles(
            [role],
            proof,
            crate::node::read_set::Scope::Consumer,
            || Ok::<(), std::convert::Infallible>(()),
        )? {
            Ok(((), [guard])) => Ok(guard),
            Err(
                transaction::Rejected::Operation(never)
                | transaction::Rejected::Cleanup { failure: never, .. },
            ) => match never {},
        }
    }

    #[cfg(test)]
    pub(crate) fn pin_live_last_good(
        &self,
        proof: &crate::node::ReadSetProof,
    ) -> Result<(pins::Guard, pins::Guard), Error> {
        match self.with_live_last_good(proof, || Ok::<(), std::convert::Infallible>(()))? {
            Ok(((), [live, last_good])) => Ok((live, last_good)),
            Err(
                transaction::Rejected::Operation(never)
                | transaction::Rejected::Cleanup { failure: never, .. },
            ) => match never {},
        }
    }

    /// Retain rollback ownership until validation and the admission callback succeed.
    /// The callback must not open a store, mutate it, or acquire execution authority.
    pub(crate) fn with_live_last_good<T, E>(
        &self,
        proof: &crate::node::ReadSetProof,
        operation: impl FnOnce() -> Result<T, E>,
    ) -> Result<transaction::Admitted<T, E, 2>, Error> {
        lease::with_project(&self.root, self.project, || {
            self.with_pin_roles(
                [PinRole::Live, PinRole::LastGood],
                proof,
                crate::node::read_set::Scope::Complete,
                operation,
            )
        })
    }

    fn with_pin_roles<T, E, const N: usize>(
        &self,
        roles: [PinRole; N],
        proof: &crate::node::ReadSetProof,
        scope: crate::node::read_set::Scope,
        operation: impl FnOnce() -> Result<T, E>,
    ) -> Result<transaction::Admitted<T, E, N>, Error> {
        proof.revalidate(self)?;
        lease::with_publication(&self.root, |publication| {
            let mut recovery = recovery::Budget::new();
            self.with_pin_roles_at(publication, roles, proof, &mut recovery, scope, operation)
        })
    }

    // Called only after a confirmed miss, while the project lease is held.
    // Keep temporary recovery and pin publication in one publication window;
    // eviction remains downstream of the parent pin.
    fn acquire_in_flight(
        &self,
        ownership: &Ownership<'_>,
        proof: Option<InFlight<'_>>,
        recovery: &mut recovery::Budget,
    ) -> Result<Option<pins::Guard>, Error> {
        ownership.require(&self.root, self.project)?;
        let Some(proof) = proof else {
            return Ok(None);
        };
        match proof {
            InFlight::Current(proof) => proof.revalidate(self)?,
            InFlight::Retained(retention) => retention.validate_storage(self)?,
        }
        let result = lease::with_publication(&self.root, |publication| {
            recovery::recover_temporaries(publication, &self.root, recovery)?;
            match proof {
                InFlight::Current(proof) => self.with_pin_roles_at(
                    publication,
                    [PinRole::InFlight],
                    proof,
                    recovery,
                    crate::node::read_set::Scope::Consumer,
                    || Ok::<(), std::convert::Infallible>(()),
                ),
                InFlight::Retained(retention) => {
                    pins::recover_empty_before_new_project(
                        publication,
                        &self.root,
                        self.project,
                        recovery,
                    )?;
                    transaction::Transaction::bind_retained(publication, self, retention)?.finish(
                        || retention.validate_storage(self),
                        || Ok::<(), std::convert::Infallible>(()),
                    )
                }
            }
        })?;
        match result {
            Ok(((), [guard])) => Ok(Some(guard)),
            Err(
                transaction::Rejected::Operation(never)
                | transaction::Rejected::Cleanup { failure: never, .. },
            ) => match never {},
        }
    }

    fn with_pin_roles_at<T, E, const N: usize>(
        &self,
        publication: &lease::Publication<'_>,
        roles: [PinRole; N],
        proof: &crate::node::ReadSetProof,
        recovery: &mut recovery::Budget,
        scope: crate::node::read_set::Scope,
        operation: impl FnOnce() -> Result<T, E>,
    ) -> Result<transaction::Admitted<T, E, N>, Error> {
        pins::recover_empty_before_new_project(publication, &self.root, self.project, recovery)?;

        transaction::Transaction::bind(publication, &self.root, self.project, roles, proof)?
            .finish(|| proof.revalidate_scoped(self, scope), operation)
    }

    pub(crate) fn candidates<K: Value>(
        &self,
        lookup: crate::node::LookupKey,
        budget: &crate::node::GraphBudget,
    ) -> Result<index::Candidates, Error> {
        index::lookup(&self.root, K::KIND, K::SCHEMA, lookup, budget)
    }

    pub(crate) fn inspect_candidate<K: Value>(
        &self,
        action: ActionKey,
        output_limit: usize,
        budget: &crate::node::GraphBudget,
    ) -> Result<Option<crate::node::replay::Replayed<K>>, Error> {
        let action_relative = self.action_relative::<K>(action);
        let Some(action_path) =
            entry::existing_directory_budgeted(&self.root, &action_relative, budget)?
        else {
            return Ok(None);
        };
        let envelope_path = action_path.join("envelope");
        let envelope_bytes =
            entry::read_candidate_file_budgeted(&envelope_path, MAX_ENVELOPE_BYTES, budget)?;
        let action_record = self.validate_allocation_record(
            &action_path,
            accounting::Class::Action,
            K::KIND,
            K::SCHEMA,
            Digest::from_bytes(action.as_bytes()),
            (
                envelope_bytes.len(),
                sha2::Sha256::digest(&envelope_bytes).into(),
            ),
            "envelope",
            Some(budget),
            accounting::Record::read_candidate_budgeted,
        )?;
        let envelope = Envelope::decode(&envelope_bytes).map_err(|_| Error::Corrupt {
            path: envelope_path,
            reason: "candidate envelope decoding failed",
        })?;
        if envelope.kind != K::KIND
            || envelope.output_schema != K::SCHEMA
            || envelope.action != action
            || envelope.output_limit != output_limit
        {
            return Err(Error::Corrupt {
                path: action_path,
                reason: "candidate envelope identity changed",
            });
        }
        let content_relative = self.content_relative::<K>(envelope.content);
        let content = entry::existing_directory_budgeted(&self.root, &content_relative, budget)?
            .ok_or_else(|| Error::Corrupt {
                path: self.root.join(&content_relative),
                reason: "candidate content-addressed directory is missing",
            })?;
        let content_path = content.join("payload");
        let before = fs::symlink_metadata(&content_path).ok();
        let bytes = entry::read_candidate_payload_budgeted(
            &content_path,
            envelope.content_length,
            output_limit,
            budget,
        )?;
        let (sha, current) = payload_digests::<K>(&content_path, before.as_ref(), &bytes);
        let content_record = self.validate_allocation_record(
            &content,
            accounting::Class::Content,
            K::KIND,
            K::SCHEMA,
            envelope.content,
            (bytes.len(), sha),
            "payload",
            Some(budget),
            accounting::Record::read_candidate_budgeted,
        )?;
        budget.hash(bytes.len())?;
        if bytes.len() != envelope.content_length || current != envelope.content {
            return Err(Error::Corrupt {
                path: content_path,
                reason: "candidate content length or digest changed",
            });
        }
        let bundled =
            accounting::registered_action(&self.root, &action_record, &content_record, budget)?;
        Stored::new(envelope, bytes, bundled)
            .inspect(output_limit, budget)
            .map(Some)
    }

    pub(crate) fn hint_candidate<K: Value>(
        &self,
        lookup: crate::node::LookupKey,
        action: ActionKey,
        repair: bool,
    ) {
        let _ = lease::with_publication(&self.root, |publication| {
            if repair
                && index::repair_existing(
                    publication,
                    &self.root,
                    K::KIND,
                    K::SCHEMA,
                    lookup,
                    action,
                    self.project,
                )?
            {
                return Ok(());
            }
            index::publish(
                publication,
                &self.root,
                K::KIND,
                K::SCHEMA,
                lookup,
                action,
                self.project,
            )
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_allocation(
        &self,
        directory: &Path,
        class: accounting::Class,
        kind: crate::node::Kind,
        schema: u32,
        key: Digest,
        payload: &[u8],
        payload_name: &'static str,
        budget: Option<&crate::node::GraphBudget>,
    ) -> Result<(), Error> {
        let record = self.validate_allocation_record(
            directory,
            class,
            kind,
            schema,
            key,
            (payload.len(), sha2::Sha256::digest(payload).into()),
            payload_name,
            budget,
            accounting::Record::read_budgeted,
        )?;
        accounting::registered_with_budget(&self.root, &record, budget)
    }

    #[allow(clippy::too_many_arguments)]
    fn validate_allocation_record(
        &self,
        directory: &Path,
        class: accounting::Class,
        kind: crate::node::Kind,
        schema: u32,
        key: Digest,
        (length, digest): (usize, [u8; 32]),
        payload_name: &'static str,
        budget: Option<&crate::node::GraphBudget>,
        read_record: fn(&Path, &crate::node::GraphBudget) -> Result<accounting::Record, Error>,
    ) -> Result<accounting::Record, Error> {
        let path = directory.join(payload_name);
        if let Some(budget) = budget {
            budget.paths(1)?;
        }
        let metadata = fs::metadata(&path).map_err(|source| Error::Io {
            operation: "inspect typed store accounted payload",
            path,
            source,
        })?;
        let record = match budget {
            Some(budget) => read_record(directory, budget),
            None => accounting::Record::read(directory),
        }
        .and_then(|record| {
            if let Some(budget) = budget {
                budget.hash(length)?;
            }
            record
                .validate_digest(
                    class,
                    kind,
                    schema,
                    key,
                    u64::try_from(length).unwrap_or(u64::MAX),
                    digest,
                    &metadata,
                )
                .map(|()| record)
        })
        .map_err(|error| match error {
            Error::Io { .. }
            | Error::Shape(_)
            | Error::OutsideRoot(_)
            | Error::Limit { .. }
            | Error::Allocation => error,
            _ => Error::Corrupt {
                path: directory.join("allocation"),
                reason: "typed store allocation seal changed",
            },
        })?;
        Ok(record)
    }

    /// `validate_allocation_record` over a payload whose length and SHA-256
    /// are already known for its current physical identity.
    #[allow(
        clippy::too_many_arguments,
        reason = "mirrors validate_allocation_record"
    )]
    fn validate_allocation_record_digest(
        &self,
        directory: &Path,
        class: accounting::Class,
        kind: crate::node::Kind,
        schema: u32,
        key: Digest,
        length: u64,
        digest: [u8; 32],
        metadata: &fs::Metadata,
        budget: &crate::node::GraphBudget,
    ) -> Result<accounting::Record, Error> {
        accounting::Record::read_budgeted(directory, budget)
            .and_then(|record| {
                record
                    .validate_digest(class, kind, schema, key, length, digest, metadata)
                    .map(|()| record)
            })
            .map_err(|error| match error {
                Error::Io { .. }
                | Error::Shape(_)
                | Error::OutsideRoot(_)
                | Error::Limit { .. }
                | Error::Allocation => error,
                _ => Error::Corrupt {
                    path: directory.join("allocation"),
                    reason: "typed store allocation seal changed",
                },
            })
    }

    pub(crate) fn revalidate_read_set_entry(
        &self,
        expected: Entry,
        budget: &crate::node::GraphBudget,
    ) -> Result<(), Error> {
        let action_relative = PathBuf::from("actions")
            .join(expected.kind.directory())
            .join(crate::node::STORAGE_DIRECTORY)
            .join(expected.action.directory());
        let action = entry::existing_directory_budgeted(&self.root, &action_relative, budget)?
            .ok_or_else(|| Error::Corrupt {
                path: self.root.join(&action_relative),
                reason: "read-set action directory is missing",
            })?;
        let envelope_path = action.join("envelope");
        let envelope_bytes = entry::read_file_budgeted(&envelope_path, MAX_ENVELOPE_BYTES, budget)?;
        let action_record = self.validate_allocation_record(
            &action,
            accounting::Class::Action,
            expected.kind,
            expected.schema,
            Digest::from_bytes(expected.action.as_bytes()),
            (
                envelope_bytes.len(),
                sha2::Sha256::digest(&envelope_bytes).into(),
            ),
            "envelope",
            Some(budget),
            accounting::Record::read_budgeted,
        )?;
        let envelope = Envelope::decode(&envelope_bytes).map_err(|_| Error::Corrupt {
            path: envelope_path,
            reason: "read-set envelope decoding failed",
        })?;
        if envelope.kind != expected.kind
            || envelope.output_schema != expected.schema
            || envelope.action != expected.action
            || envelope.recipe != expected.recipe
            || envelope.content != expected.content
        {
            return Err(Error::StaleReadSet {
                action: expected.action,
            });
        }
        let content_relative = PathBuf::from("content")
            .join(expected.kind.directory())
            .join(format!("{:08x}", expected.schema))
            .join(expected.content.directory());
        let content = entry::existing_directory_budgeted(&self.root, &content_relative, budget)?
            .ok_or_else(|| Error::Corrupt {
                path: self.root.join(&content_relative),
                reason: "read-set content directory is missing",
            })?;
        let payload = content.join("payload");
        // Both payload digests are pure functions of its bytes: computed once
        // per physical identity and reused by every later revalidation.
        let metadata = entry::inspect_payload_budgeted(
            &payload,
            envelope.content_length,
            envelope.output_limit,
            budget,
        )?;
        let identity = crate::file_digest::Identity::of(&metadata);
        let content_domain = crate::file_digest::Domain::Content {
            kind: expected.kind as u8,
            schema: expected.schema,
        };
        let memoized = crate::file_digest::get(identity, crate::file_digest::Domain::Sha256)
            .zip(crate::file_digest::get(identity, content_domain));
        let (sha, current) = match memoized {
            Some(digests) => digests,
            None => {
                let bytes = entry::read_payload_budgeted(
                    &payload,
                    envelope.content_length,
                    envelope.output_limit,
                    budget,
                )?;
                budget.hash(bytes.len())?;
                budget.hash(bytes.len())?;
                let sha: [u8; 32] = sha2::Sha256::digest(&bytes).into();
                let current =
                    content_digest_parts(expected.kind, expected.schema, &bytes).as_bytes();
                let after = fs::symlink_metadata(&payload).ok();
                if after.as_ref().and_then(crate::file_digest::Identity::of) == identity {
                    crate::file_digest::insert(identity, crate::file_digest::Domain::Sha256, sha);
                    crate::file_digest::insert(identity, content_domain, current);
                }
                (sha, current)
            }
        };
        let content_record = self.validate_allocation_record_digest(
            &content,
            accounting::Class::Content,
            expected.kind,
            expected.schema,
            expected.content,
            metadata.len(),
            sha,
            &metadata,
            budget,
        )?;
        if metadata.len() != envelope.content_length as u64
            || Digest::from_bytes(current) != expected.content
        {
            return Err(Error::Corrupt {
                path: payload,
                reason: "read-set content length or digest changed",
            });
        }
        if accounting::registered_action(&self.root, &action_record, &content_record, budget)?
            != expected.bundled
        {
            return Err(Error::StaleReadSet {
                action: expected.action,
            });
        }
        Ok(())
    }

    pub(crate) fn with_action_lease<T>(
        &self,
        action: ActionKey,
        evaluate: impl FnOnce() -> Result<T, Error>,
    ) -> Result<T, Error> {
        lease::with_action(&self.root, action, evaluate)
    }

    pub(crate) fn with_heavy_lease<T>(
        &self,
        evaluate: impl FnOnce() -> Result<T, Error>,
    ) -> Result<T, Error> {
        lease::with_heavy(&self.root, evaluate)
    }

    pub(crate) fn with_execution_lease<T>(
        &self,
        execution: crate::node::ExecutionKey,
        evaluate: impl FnOnce() -> Result<T, Error>,
    ) -> Result<T, Error> {
        lease::with_execution(&self.root, execution, evaluate)
    }

    fn action_path<K: Value>(&self, action: ActionKey) -> PathBuf {
        self.root.join(self.action_relative::<K>(action))
    }

    fn action_relative<K: Value>(&self, action: ActionKey) -> PathBuf {
        action_relative(K::KIND, action)
    }

    fn content_parent<K: Value>(&self) -> PathBuf {
        PathBuf::from("content")
            .join(K::KIND.directory())
            .join(format!("{:08x}", K::SCHEMA))
    }

    fn content_path<K: Value>(&self, digest: crate::node::Digest) -> PathBuf {
        self.root.join(self.content_relative::<K>(digest))
    }

    fn content_relative<K: Value>(&self, digest: crate::node::Digest) -> PathBuf {
        content_relative(K::KIND, K::SCHEMA, digest)
    }
}

// Shared with inert principal evidence; these remain the production namespaces.
pub(crate) fn action_relative(kind: Kind, action: ActionKey) -> PathBuf {
    PathBuf::from("actions")
        .join(kind.directory())
        .join(crate::node::STORAGE_DIRECTORY)
        .join(action.directory())
}
pub(crate) fn content_relative(kind: Kind, schema: u32, digest: Digest) -> PathBuf {
    PathBuf::from("content")
        .join(kind.directory())
        .join(format!("{schema:08x}"))
        .join(digest.directory())
}

/// Both digests of one content payload just read. Each is a pure function of
/// the bytes, so they are remembered for the physical identity the payload kept
/// across its read and every later read-set revalidation reuses them.
fn payload_digests<K: Value>(
    path: &Path,
    before: Option<&fs::Metadata>,
    bytes: &[u8],
) -> ([u8; 32], Digest) {
    let identity = before.and_then(crate::file_digest::Identity::of);
    let after = fs::symlink_metadata(path).ok();
    let unchanged =
        identity.is_some() && after.as_ref().and_then(crate::file_digest::Identity::of) == identity;
    let domain = crate::file_digest::Domain::Content {
        kind: K::KIND as u8,
        schema: K::SCHEMA,
    };
    if unchanged
        && let Some(sha) = crate::file_digest::get(identity, crate::file_digest::Domain::Sha256)
        && let Some(current) = crate::file_digest::get(identity, domain)
    {
        return (sha, Digest::from_bytes(current));
    }
    let sha: [u8; 32] = sha2::Sha256::digest(bytes).into();
    let current = content_digest::<K>(bytes);
    if unchanged {
        crate::file_digest::insert(identity, crate::file_digest::Domain::Sha256, sha);
        crate::file_digest::insert(identity, domain, current.as_bytes());
    }
    (sha, current)
}

#[cfg(test)]
mod payload_digest_tests {
    use sha2::Digest as _;

    #[test]
    fn an_unchanged_payload_reuses_its_digests_and_a_changed_one_is_hashed() {
        type Kind = crate::api::cached::Product;
        let directory =
            std::env::temp_dir().join(format!("rawr-payload-digests-{}", std::process::id()));
        std::fs::create_dir_all(&directory).expect("temporary directory");
        let path = directory.join("payload");
        std::fs::write(&path, b"first").expect("payload is written");
        let before = std::fs::symlink_metadata(&path).ok();
        let first = super::payload_digests::<Kind>(&path, before.as_ref(), b"first");
        assert_eq!(first.0, <[u8; 32]>::from(sha2::Sha256::digest(b"first")));
        // Same identity: the remembered digests answer without hashing `bytes`.
        assert!(super::payload_digests::<Kind>(&path, before.as_ref(), b"other") == first);
        std::fs::write(&path, b"other").expect("payload is rewritten");
        let after = std::fs::symlink_metadata(&path).ok();
        let second = super::payload_digests::<Kind>(&path, after.as_ref(), b"other");
        assert_eq!(second.0, <[u8; 32]>::from(sha2::Sha256::digest(b"other")));
        assert!(second.1 != first.1);
        std::fs::remove_dir_all(&directory).expect("temporary directory is removed");
    }
}

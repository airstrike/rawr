//! Exact physical inventory outside registered immutable node allocations.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use super::accounting::{Charge, Class, Record, Registered, Reservation};
use crate::node::{Error, ExecutionKey};

const BLOCK_BYTES: u64 = 4 * 1024;

#[derive(Clone, Copy, Eq, PartialEq)]
enum Operation {
    Directory,
    File,
    Publication,
    Replacement,
    Retirement,
}

pub(super) struct Plan {
    operation: Operation,
    target: PathBuf,
    length: usize,
}

pub(super) struct Admitted<'lease> {
    publication: &'lease super::lease::Publication<'lease>,
    plan: Plan,
}

impl Plan {
    pub(super) fn directory(target: &Path) -> Self {
        Self {
            operation: Operation::Directory,
            target: target.to_path_buf(),
            length: 0,
        }
    }

    pub(super) fn file(target: &Path) -> Self {
        Self {
            operation: Operation::File,
            target: target.to_path_buf(),
            length: 0,
        }
    }

    pub(super) fn publication(target: &Path, length: usize) -> Self {
        Self {
            operation: Operation::Publication,
            target: target.to_path_buf(),
            length,
        }
    }

    pub(super) fn replacement(target: &Path, length: usize) -> Self {
        Self {
            operation: Operation::Replacement,
            target: target.to_path_buf(),
            length,
        }
    }

    pub(super) fn retirement(target: &Path) -> Self {
        Self {
            operation: Operation::Retirement,
            target: target.to_path_buf(),
            length: 0,
        }
    }

    pub(super) fn retirement_target(&self) -> Option<&Path> {
        (self.operation == Operation::Retirement).then_some(self.target.as_path())
    }

    fn creates_project_ledger(&self, root: &Path) -> Result<bool, Error> {
        if self.operation != Operation::Replacement {
            return Ok(false);
        }
        let relative = self
            .target
            .strip_prefix(root)
            .map_err(|_| Error::OutsideRoot(self.target.clone()))?;
        if !project_ledger_position(relative) {
            return Ok(false);
        }
        match fs::symlink_metadata(&self.target) {
            Ok(_) => Ok(false),
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => Ok(true),
            Err(source) => Err(Error::Io {
                operation: "inspect planned project ledger",
                path: self.target.clone(),
                source,
            }),
        }
    }
}

impl<'lease> Admitted<'lease> {
    pub(super) fn seal(
        publication: &'lease super::lease::Publication<'lease>,
        root: &Path,
        plan: Plan,
    ) -> Result<Self, Error> {
        if publication.root() != root || !plan.target.starts_with(root) {
            return Err(Error::OutsideRoot(plan.target));
        }
        let relative = plan
            .target
            .strip_prefix(root)
            .map_err(|_| Error::State("typed store control plan lost its canonical root"))?;
        if !valid_plan(relative, plan.operation) {
            return Err(Error::State(
                "typed store control plan is outside the closed path grammar",
            ));
        }
        Ok(Self { publication, plan })
    }

    pub(super) fn consume(
        self,
        publication: &super::lease::Publication<'_>,
        root: &Path,
        operation: &'static str,
        target: &Path,
        length: usize,
    ) -> Result<(), Error> {
        let expected = match operation {
            "directory" => Operation::Directory,
            "file" => Operation::File,
            "publication" => Operation::Publication,
            "replacement" => Operation::Replacement,
            "retirement" => Operation::Retirement,
            _ => {
                return Err(Error::State(
                    "typed store control mutation changed operation class",
                ));
            }
        };
        if !std::ptr::eq(self.publication, publication)
            || publication.root() != root
            || self.plan.operation != expected
            || self.plan.target != target
            || self.plan.length != length
        {
            return Err(Error::State(
                "typed store control mutation changed its admitted plan",
            ));
        }
        Ok(())
    }
}

pub(super) fn inventory(
    root: &Path,
    registrations: &[Registered],
    evictions: &[Registered],
    reservations: &[Reservation],
) -> Result<Charge, Error> {
    inspect(root, registrations, evictions, reservations, None, None).map(|state| state.control)
}

/// Admit one control mutation by what it changes, not by re-inventorying the
/// store: a retirement checks its target bundle is journaled for
/// eviction and intact, and a new project ledger is counted against the limit.
/// Every allocation a consumer reads is validated by that consumer, so walking
/// the whole store on each write only repeated their checks.
pub(super) fn admit_plan(
    root: &Path,
    registrations: &[Registered],
    plan: &Plan,
) -> Result<(), Error> {
    if let Some(target) = plan.retirement_target() {
        let relative = target
            .strip_prefix(root)
            .map_err(|_| Error::OutsideRoot(target.to_path_buf()))?;
        if bundle_execution(relative).is_some()
            && let Ok(record) = Record::read_bundle(target)
        {
            let allocation = record.allocation();
            let registered = |entries: &[Registered]| {
                entries
                    .iter()
                    .any(|registered| registered.allocation() == allocation)
            };
            if registered(registrations) {
                return Err(Error::CleanupRequired(
                    "typed store retirement targeted a bundle outside the eviction journal",
                ));
            }
        }
    }
    if plan.creates_project_ledger(root)? {
        let projects = root.join("ledgers").join("projects");
        let mut ledgers = 0;
        match fs::read_dir(&projects) {
            Ok(entries) => {
                for entry in entries {
                    let entry = entry.map_err(|source| Error::Io {
                        operation: "read typed store project ledgers",
                        path: projects.clone(),
                        source,
                    })?;
                    let relative = Path::new("ledgers/projects").join(entry.file_name());
                    if project_ledger_position(&relative) {
                        admit_project_ledger(&mut ledgers)?;
                    }
                }
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(Error::Io {
                    operation: "read typed store project ledgers",
                    path: projects,
                    source,
                })
            }
        }
        admit_project_ledger(&mut ledgers)?;
    }
    Ok(())
}

pub(super) fn inventory_for_repair(
    root: &Path,
    registrations: &[Registered],
    evictions: &[Registered],
    reservations: &[Reservation],
    plan: &Plan,
    repair: &super::bundle::Repair,
) -> Result<Charge, Error> {
    if !evictions
        .iter()
        .any(|entry| entry.allocation() == repair.allocation())
    {
        return Err(Error::State(
            "bundle repair has no exact eviction journal identity",
        ));
    }
    let state = inspect(
        root,
        registrations,
        evictions,
        reservations,
        plan.retirement_target(),
        Some(repair),
    )?;
    if !state
        .observed
        .iter()
        .any(|entry| entry.allocation() == repair.allocation())
    {
        return Err(Error::CleanupRequired(
            "bundle repair lost its measured target",
        ));
    }
    Ok(state.control)
}

pub(super) fn garbage(
    root: &Path,
    registrations: &[Registered],
    evictions: &[Registered],
    reservations: &[Reservation],
) -> Result<Option<(PathBuf, Charge)>, Error> {
    inspect(root, registrations, evictions, reservations, None, None).map(|state| state.garbage)
}

fn inspect<'store>(
    root: &'store Path,
    registrations: &[Registered],
    evictions: &[Registered],
    reservations: &[Reservation],
    retirement: Option<&Path>,
    repair: Option<&'store super::bundle::Repair>,
) -> Result<State<'store>, Error> {
    let mut immutable = BTreeMap::new();
    let mut bundles = BTreeMap::new();
    for (registered, required) in registrations
        .iter()
        .copied()
        .map(|registered| (registered, true))
        .chain(
            evictions
                .iter()
                .copied()
                .map(|registered| (registered, false)),
        )
    {
        let allocation = registered.allocation();
        match allocation.class {
            Class::Action => {
                let path = PathBuf::from("actions")
                    .join(allocation.kind.directory())
                    .join(crate::node::STORAGE_DIRECTORY)
                    .join(allocation.key.directory());
                insert(&mut immutable, path, registered, required)?;
            }
            Class::Content => {
                let path = PathBuf::from("content")
                    .join(allocation.kind.directory())
                    .join(format!("{:08x}", allocation.schema))
                    .join(allocation.key.directory());
                insert(&mut immutable, path, registered, required)?;
            }
            Class::Bundle => {
                if bundles.insert(registered, required).is_some() {
                    return Err(Error::CleanupRequired(
                        "typed store physical bundle registry resolved ambiguously",
                    ));
                }
            }
        }
    }
    let active_executions = reservations
        .iter()
        .filter_map(|reservation| {
            reservation
                .execution()
                .map(|execution| (execution, reservation.charge()))
        })
        .collect::<BTreeMap<_, _>>();
    let mut planned = BTreeSet::new();
    for reservation in reservations {
        let Some((kind, schema, action, content, _bundle)) = reservation.planned() else {
            continue;
        };
        planned.insert(
            PathBuf::from("actions")
                .join(kind.directory())
                .join(crate::node::STORAGE_DIRECTORY)
                .join(action.directory()),
        );
        planned.insert(
            PathBuf::from("content")
                .join(kind.directory())
                .join(format!("{schema:08x}"))
                .join(content.directory()),
        );
    }
    let mut state = State {
        root,
        immutable,
        bundles,
        active_executions,
        planned,
        observed_active: BTreeSet::new(),
        observed: BTreeSet::new(),
        control: Charge::zero(),
        garbage: None,
        retirement: retirement.map(Path::to_path_buf),
        repair,
        project_ledgers: 0,
    };
    state.walk(Path::new(""))?;
    if registrations
        .iter()
        .any(|registered| !state.observed.contains(registered))
    {
        return Err(Error::CleanupRequired(
            "typed store physical inventory lost a registered allocation",
        ));
    }
    Ok(state)
}

fn insert(
    immutable: &mut BTreeMap<PathBuf, (Registered, bool)>,
    path: PathBuf,
    registered: Registered,
    required: bool,
) -> Result<(), Error> {
    if immutable.insert(path, (registered, required)).is_some() {
        return Err(Error::CleanupRequired(
            "typed store physical allocation placement resolved ambiguously",
        ));
    }
    Ok(())
}

struct State<'store> {
    root: &'store Path,
    immutable: BTreeMap<PathBuf, (Registered, bool)>,
    bundles: BTreeMap<Registered, bool>,
    active_executions: BTreeMap<ExecutionKey, Charge>,
    planned: BTreeSet<PathBuf>,
    observed_active: BTreeSet<ExecutionKey>,
    observed: BTreeSet<Registered>,
    control: Charge,
    garbage: Option<(PathBuf, Charge)>,
    retirement: Option<PathBuf>,
    repair: Option<&'store super::bundle::Repair>,
    project_ledgers: usize,
}

impl State<'_> {
    fn walk(&mut self, relative: &Path) -> Result<(), Error> {
        if let Some((registered, _required)) = self.immutable.get(relative).copied() {
            return self.allocation(relative, registered);
        }
        if let Some(execution) = bundle_execution(relative) {
            return self.bundle(relative, execution);
        }
        if allocation_position(relative) {
            if self.planned.contains(relative) {
                let path = self.root.join(relative);
                let metadata = fs::symlink_metadata(&path).map_err(|source| Error::Io {
                    operation: "inspect typed store reserved allocation root",
                    path: path.clone(),
                    source,
                })?;
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(Error::Shape(path));
                }
                return Ok(());
            }
            return self.garbage(relative);
        }
        if invalid_bundle_position(relative) {
            return Err(Error::CleanupRequired(
                "typed store physical inventory found a malformed bundle placement",
            ));
        }
        let path = self.root.join(relative);
        let metadata = fs::symlink_metadata(&path).map_err(|source| Error::Io {
            operation: "inspect typed store physical control member",
            path: path.clone(),
            source,
        })?;
        if metadata.file_type().is_symlink() {
            return Err(Error::Shape(path));
        }
        if !valid_existing(relative, &metadata) {
            return Err(Error::CleanupRequired(
                "typed store physical inventory found a path outside the closed grammar",
            ));
        }
        if metadata.is_dir() {
            self.control = self.control.checked_add(directory_charge())?;
            let directory = fs::read_dir(&path).map_err(|source| Error::Io {
                operation: "read typed store physical control directory",
                path: path.clone(),
                source,
            })?;
            let mut children = Vec::new();
            for child in directory {
                children.try_reserve(1).map_err(|_| Error::Allocation)?;
                children.push(child.map_err(|source| Error::Io {
                    operation: "read typed store physical control member",
                    path: path.clone(),
                    source,
                })?);
            }
            children.sort_unstable_by_key(fs::DirEntry::file_name);
            if relative == Path::new("temporaries") {
                // An interrupted run's temporaries: charged here, removed by
                // the next temporary recovery.
                for child in children {
                    self.dead(child.path())?;
                }
                return Ok(());
            }
            for child in children {
                self.walk(&relative.join(child.file_name()))?;
            }
            Ok(())
        } else if metadata.is_file() && !hard_linked(&metadata) {
            if project_ledger_position(relative) {
                admit_project_ledger(&mut self.project_ledgers)?;
            }
            self.control = self
                .control
                .checked_add(file_charge_from_metadata(&metadata)?)?;
            Ok(())
        } else {
            Err(Error::Shape(path))
        }
    }

    fn allocation(&mut self, relative: &Path, registered: Registered) -> Result<(), Error> {
        let path = self.root.join(relative);
        let metadata = fs::symlink_metadata(&path).map_err(|source| Error::Io {
            operation: "inspect typed store immutable allocation root",
            path: path.clone(),
            source,
        })?;
        if metadata.file_type().is_symlink()
            || !metadata.is_dir()
            || registered.allocation().charge.allocations != 1
        {
            return Err(Error::Shape(path));
        }
        // A registered allocation is frozen when published and its record and
        // payload are checked by every consumer that reads them; the inventory
        // only needs its placement, as for bundles. Re-reading every
        // record on every store write made each write cost the whole store.
        if !self.observed.insert(registered) {
            return Err(Error::CleanupRequired(
                "typed store physical inventory counted an allocation twice",
            ));
        }
        Ok(())
    }

    fn bundle(&mut self, relative: &Path, execution: ExecutionKey) -> Result<(), Error> {
        let root = self.root.join(relative);
        let allocation = match Record::read_bundle(&root) {
            Ok(record) => record.allocation(),
            Err(_) if self.active_executions.contains_key(&execution) => {
                return self.observe_active(execution)
            }
            // An interrupted publication's remains: charged here, removed by
            // dead-bundle recovery once its execution lease is free.
            Err(_) => return self.dead(root).map(drop),
        };
        let registered = self
            .bundles
            .iter()
            .find(|(registered, _)| registered.allocation() == allocation)
            .map(|(registered, required)| (*registered, *required));
        let Some((registered, required)) = registered else {
            if self.active_executions.contains_key(&execution) {
                return self.observe_active(execution);
            }
            return self.dead(root).map(drop);
        };
        if allocation.charge.allocations != 1 {
            return Err(Error::CleanupRequired(
                "typed store registered bundle changed physical charge",
            ));
        }
        if let Some(repair) = self.repair.filter(|repair| repair.root() == root) {
            if required {
                return Err(Error::CleanupRequired(
                    "bundle repair is outside the eviction journal",
                ));
            }
            self.control = self
                .control
                .checked_add(repair.validate(&root, allocation)?)?;
        } else if self.retirement.as_deref() == Some(root.as_path()) && required {
            // A bundle journaled for eviction is retired as it stands: its
            // tree may differ from its seal, and it is being deleted anyway.
            return Err(Error::CleanupRequired(
                "typed store retirement targeted a bundle outside the eviction journal",
            ));
        }
        // Otherwise the record names a registered allocation, which is all the
        // inventory needs: a bundle's tree is checked when it is sealed and
        // published, and again by `validate_complete` whenever a consumer uses
        // it. Re-walking every frozen bundle on every store write made each
        // write cost the whole store.
        if !self.observed.insert(registered) {
            return Err(Error::CleanupRequired(
                "typed store physical inventory counted a bundle twice",
            ));
        }
        Ok(())
    }

    fn garbage(&mut self, relative: &Path) -> Result<(), Error> {
        let root = self.root.join(relative);
        let root_metadata = fs::symlink_metadata(&root).map_err(|source| Error::Io {
            operation: "inspect typed store legacy or unregistered allocation root",
            path: root.clone(),
            source,
        })?;
        if root_metadata.file_type().is_symlink() || !root_metadata.is_dir() {
            return Err(Error::Shape(root));
        }
        let total = self.dead(root)?;
        if self.garbage.is_none() {
            self.garbage = Some((self.root.join(relative), total));
        }
        Ok(())
    }

    /// Charge one unregistered tree (directory or file) as control.
    fn dead(&mut self, root: PathBuf) -> Result<Charge, Error> {
        let mut pending = Vec::new();
        pending.try_reserve(1).map_err(|_| Error::Allocation)?;
        pending.push(root);
        let mut total = Charge::zero();
        while let Some(path) = pending.pop() {
            let metadata = fs::symlink_metadata(&path).map_err(|source| Error::Io {
                operation: "inspect typed store legacy or unregistered allocation",
                path: path.clone(),
                source,
            })?;
            if metadata.file_type().is_symlink() {
                return Err(Error::Shape(path));
            }
            let charge = if metadata.is_dir() {
                let directory = fs::read_dir(&path).map_err(|source| Error::Io {
                    operation: "read typed store legacy or unregistered allocation",
                    path: path.clone(),
                    source,
                })?;
                for child in directory {
                    pending.try_reserve(1).map_err(|_| Error::Allocation)?;
                    pending.push(
                        child
                            .map_err(|source| Error::Io {
                                operation:
                                    "read typed store legacy or unregistered allocation member",
                                path: path.clone(),
                                source,
                            })?
                            .path(),
                    );
                }
                directory_charge()
            } else if metadata.is_file() {
                file_charge_from_metadata(&metadata)?
            } else {
                return Err(Error::Shape(path));
            };
            total = total.checked_add(charge)?;
            self.control = self.control.checked_add(charge)?;
        }
        Ok(total)
    }

    fn observe_active(&mut self, execution: ExecutionKey) -> Result<(), Error> {
        if !self.observed_active.insert(execution) {
            return Err(Error::CleanupRequired(
                "typed store reservation owns more than one unregistered bundle",
            ));
        }
        Ok(())
    }
}

fn admit_project_ledger(project_ledgers: &mut usize) -> Result<(), Error> {
    *project_ledgers = project_ledgers.checked_add(1).ok_or(Error::Allocation)?;
    if *project_ledgers > super::pins::MAX_PROJECT_LEDGERS {
        return Err(Error::Limit {
            resource: "typed store installation project ledgers",
            limit: super::pins::MAX_PROJECT_LEDGERS,
            actual: *project_ledgers,
        });
    }
    Ok(())
}

fn allocation_position(relative: &Path) -> bool {
    let parts = relative.iter().collect::<Vec<_>>();
    matches!(
        parts.as_slice(),
        [first, kind, schema, digest]
            if ((*first == "actions" && lower_action_schema(schema))
                || (*first == "content" && lower_schema(schema)))
                && kind_directory(kind)
                && decode_digest(digest).is_some()
    )
}

fn bundle_execution(relative: &Path) -> Option<ExecutionKey> {
    let parts = relative.iter().collect::<Vec<_>>();
    let [first, execution, _bundle] = parts.as_slice() else {
        return None;
    };
    if *first != "executions" {
        return None;
    }
    decode_digest(_bundle)?;
    decode_digest(execution).map(ExecutionKey::from_bytes)
}

fn invalid_bundle_position(relative: &Path) -> bool {
    let parts = relative.iter().collect::<Vec<_>>();
    match parts.as_slice() {
        [first, execution] if *first == "executions" => decode_digest(execution).is_none(),
        [first, _, _] if *first == "executions" => bundle_execution(relative).is_none(),
        [first, _, _, ..] if *first == "executions" => true,
        _ => false,
    }
}

fn valid_plan(relative: &Path, operation: Operation) -> bool {
    let parts = relative.iter().collect::<Vec<_>>();
    match operation {
        Operation::Directory => match parts.as_slice() {
            [first, class] if *first == "locks" && *class == "owners" => true,
            [first, kind]
                if (*first == "actions" || *first == "content" || *first == "indexes")
                    && kind_directory(kind) =>
            {
                true
            }
            [first, kind, schema]
                if *first == "actions"
                    && kind_directory(kind)
                    && *schema == crate::node::STORAGE_DIRECTORY =>
            {
                true
            }
            [first, kind, schema]
                if *first == "content" && kind_directory(kind) && lower_schema(schema) =>
            {
                true
            }
            [first, execution] if *first == "executions" => decode_digest(execution).is_some(),
            [first, kind, lookup]
                if *first == "indexes"
                    && kind_directory(kind)
                    && decode_digest(lookup).is_some() =>
            {
                true
            }
            _ => false,
        },
        Operation::File => {
            project_pending_position(relative)
                || matches!(
                    parts.as_slice(),
                    [first, class, key]
                        if *first == "locks"
                            && matches!(class.to_str(), Some("actions" | "executions" | "projects" | "owners"))
                            && lower_lock(key)
                )
        }
        Operation::Publication => match parts.as_slice() {
            [first, kind, lookup, action]
                if *first == "indexes"
                    && kind_directory(kind)
                    && decode_digest(lookup).is_some()
                    && decode_digest(action).is_some() =>
            {
                true
            }
            [first, role, owner]
                if *first == "pins" && pin_role(role) && decode_digest(owner).is_some() =>
            {
                true
            }
            _ => false,
        },
        Operation::Replacement => match parts.as_slice() {
            [first, ledger] if *first == "ledgers" && *ledger == "installation" => true,
            [first, projects, project]
                if *first == "ledgers"
                    && *projects == "projects"
                    && decode_digest(project).is_some() =>
            {
                true
            }
            _ => false,
        },
        Operation::Retirement => {
            allocation_position(relative)
                || bundle_execution(relative).is_some()
                || pin_root_position(relative)
                || project_ledger_position(relative)
                || project_pending_position(relative)
        }
    }
}

fn project_pending_position(relative: &Path) -> bool {
    let parts = relative.iter().collect::<Vec<_>>();
    matches!(parts.as_slice(), [first, class, name]
    if *first == "locks" && *class == "projects" && {
        let bytes = name.as_encoded_bytes();
        bytes.len() == 72 && bytes[64..] == *b".pending"
            && decode_digest_bytes(&bytes[..64]).is_some()
    })
}

fn pin_root_position(relative: &Path) -> bool {
    let parts = relative.iter().collect::<Vec<_>>();
    matches!(parts.as_slice(), [first, role, owner]
        if *first == "pins" && pin_role(role) && decode_digest(owner).is_some())
}

fn project_ledger_position(relative: &Path) -> bool {
    let parts = relative.iter().collect::<Vec<_>>();
    matches!(parts.as_slice(), [first, projects, project]
        if *first == "ledgers" && *projects == "projects" && decode_digest(project).is_some())
}

fn valid_existing(relative: &Path, metadata: &fs::Metadata) -> bool {
    let parts = relative.iter().collect::<Vec<_>>();
    let directory = metadata.is_dir();
    let file = metadata.is_file() && !hard_linked(metadata);
    if project_pending_position(relative) {
        return file && metadata.len() == 0;
    }
    match parts.as_slice() {
        [] => directory,
        [name]
            if matches!(
                name.to_str(),
                Some(
                    "actions"
                        | "content"
                        | "executions"
                        | "indexes"
                        | "ledgers"
                        | "locks"
                        | "pins"
                        | "temporaries"
                )
            ) =>
        {
            directory
        }
        [bootstrap] if *bootstrap == "bootstrap.lock" => file,
        [first, kind]
            if (*first == "actions" || *first == "content" || *first == "indexes")
                && kind_directory(kind) =>
        {
            directory
        }
        [first, kind, schema]
            if *first == "actions" && kind_directory(kind) && lower_action_schema(schema) =>
        {
            directory
        }
        [first, kind, schema]
            if *first == "content" && kind_directory(kind) && lower_schema(schema) =>
        {
            directory
        }
        [first, execution] if *first == "executions" && decode_digest(execution).is_some() => {
            directory
        }
        [first, kind, lookup]
            if *first == "indexes" && kind_directory(kind) && decode_digest(lookup).is_some() =>
        {
            directory
        }
        [first, kind, lookup, action]
            if *first == "indexes"
                && kind_directory(kind)
                && decode_digest(lookup).is_some()
                && decode_digest(action).is_some() =>
        {
            directory
        }
        [first, kind, lookup, action, row]
            if *first == "indexes"
                && kind_directory(kind)
                && decode_digest(lookup).is_some()
                && decode_digest(action).is_some()
                && *row == "row" =>
        {
            file
        }
        [first, projects] if *first == "ledgers" && *projects == "projects" => directory,
        [first, installation] if *first == "ledgers" && *installation == "installation" => file,
        [first, projects, project]
            if *first == "ledgers"
                && *projects == "projects"
                && decode_digest(project).is_some() =>
        {
            file
        }
        [first, class]
            if *first == "locks"
                && matches!(
                    class.to_str(),
                    Some("actions" | "executions" | "projects" | "owners")
                ) =>
        {
            directory
        }
        [first, lock]
            if *first == "locks"
                && matches!(lock.to_str(), Some("heavy.lock" | "publication.lock")) =>
        {
            file
        }
        [first, class, key]
            if *first == "locks"
                && matches!(
                    class.to_str(),
                    Some("actions" | "executions" | "projects" | "owners")
                )
                && lower_lock(key) =>
        {
            file
        }
        [first, role] if *first == "pins" && pin_role(role) => directory,
        [first, role, owner]
            if *first == "pins" && pin_role(role) && decode_digest(owner).is_some() =>
        {
            directory
        }
        [first, role, owner, record]
            if *first == "pins"
                && pin_role(role)
                && decode_digest(owner).is_some()
                && *record == "root" =>
        {
            file
        }
        _ => false,
    }
}

fn kind_directory(value: &std::ffi::OsStr) -> bool {
    (1..=11).any(|code| {
        crate::node::Kind::from_code(code).is_some_and(|kind| value == kind.directory())
    })
}

fn lower_schema(value: &std::ffi::OsStr) -> bool {
    let bytes = value.as_encoded_bytes();
    bytes.len() == 8
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn lower_action_schema(value: &std::ffi::OsStr) -> bool {
    let bytes = value.as_encoded_bytes();
    bytes.len() == 4
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
}

fn lower_lock(value: &std::ffi::OsStr) -> bool {
    let bytes = value.as_encoded_bytes();
    bytes.len() == 69 && bytes[64..] == *b".lock" && decode_digest_bytes(&bytes[..64]).is_some()
}

fn pin_role(value: &std::ffi::OsStr) -> bool {
    matches!(
        value.to_str(),
        Some("live" | "last-good" | "prepared" | "in-flight")
    )
}

fn decode_digest(value: &std::ffi::OsStr) -> Option<[u8; 32]> {
    decode_digest_bytes(value.as_encoded_bytes())
}

fn decode_digest_bytes(encoded: &[u8]) -> Option<[u8; 32]> {
    if encoded.len() != 64 {
        return None;
    }
    let mut digest = [0_u8; 32];
    for (index, pair) in encoded.as_chunks::<2>().0.iter().enumerate() {
        digest[index] = decode_nibble(pair[0])?.checked_mul(16)? | decode_nibble(pair[1])?;
    }
    Some(digest)
}

fn decode_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

pub(super) const fn directory_charge() -> Charge {
    Charge {
        bytes: BLOCK_BYTES,
        allocations: 0,
        members: 1,
    }
}

pub(super) fn file_charge(length: usize) -> Result<Charge, Error> {
    let length = u64::try_from(length).map_err(|_| Error::Allocation)?;
    Ok(Charge {
        bytes: round_block(length)?.max(BLOCK_BYTES),
        allocations: 0,
        members: 1,
    })
}

fn file_charge_from_metadata(metadata: &fs::Metadata) -> Result<Charge, Error> {
    Ok(Charge {
        bytes: round_block(metadata.len())?
            .max(round_block(allocated_bytes(metadata)?)?)
            .max(BLOCK_BYTES),
        allocations: 0,
        members: 1,
    })
}

fn round_block(bytes: u64) -> Result<u64, Error> {
    bytes
        .checked_add(BLOCK_BYTES - 1)
        .map(|bytes| bytes / BLOCK_BYTES * BLOCK_BYTES)
        .ok_or(Error::Allocation)
}

#[cfg(unix)]
fn allocated_bytes(metadata: &fs::Metadata) -> Result<u64, Error> {
    use std::os::unix::fs::MetadataExt as _;

    metadata.blocks().checked_mul(512).ok_or(Error::Allocation)
}

#[cfg(not(unix))]
fn allocated_bytes(metadata: &fs::Metadata) -> Result<u64, Error> {
    Ok(metadata.len())
}

#[cfg(unix)]
fn hard_linked(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;

    metadata.nlink() != 1
}

#[cfg(not(unix))]
fn hard_linked(_metadata: &fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(label: &str) -> crate::store::Store {
        let root =
            std::env::temp_dir().join(format!("rawr-store-control-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).expect("the control fixture root exists");
        crate::store::Store::open(&root, [0x93; 32]).expect("the control fixture store opens")
    }

    #[test]
    fn digest_decoder_accepts_only_exact_lower_hex() {
        assert_eq!(
            decode_digest(std::ffi::OsStr::new(
                "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f"
            )),
            Some([
                0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22,
                23, 24, 25, 26, 27, 28, 29, 30, 31,
            ])
        );
        assert!(decode_digest(std::ffi::OsStr::new("00")).is_none());
        assert!(decode_digest(std::ffi::OsStr::new(
            "000102030405060708090A0B0C0D0E0F101112131415161718191A1B1C1D1E1F"
        ))
        .is_none());
    }

    #[test]
    fn bootstrap_inventory_charges_every_fixed_path_once() {
        let store = fixture("bootstrap");
        let (allocations, control) = super::super::accounting::physical_usage(store.root())
            .expect("the bootstrap inventory reads");
        assert_eq!(allocations, Charge::zero());
        // Root + 17 fixed directories, then six retained files: bootstrap,
        // heavy/publication locks, installation ledger, and this project's
        // project/owner locks. Enrollment's pending file retires.
        assert_eq!(
            control,
            Charge {
                bytes: 24 * BLOCK_BYTES,
                allocations: 0,
                members: 24,
            }
        );
        store
            .retire_fixture()
            .expect("the bootstrap fixture retires");
    }

    #[test]
    fn keyed_lock_is_admitted_once_and_existing_acquisition_is_read_only() {
        let store = fixture("keyed-lock");
        let action = crate::node::ActionKey::from_bytes([0x51; 32]);
        let before = super::super::accounting::physical_usage(store.root())
            .expect("the initial inventory reads");
        store
            .with_action_lease(action, || Ok(()))
            .expect("the first keyed lease is admitted");
        let admitted = super::super::accounting::physical_usage(store.root())
            .expect("the admitted inventory reads");
        assert_eq!(admitted.0, before.0);
        assert_eq!(
            admitted.1,
            before
                .1
                .checked_add(file_charge(0).expect("the keyed lock charge seals"))
                .expect("the keyed lock inventory adds")
        );
        store
            .with_action_lease(action, || Ok(()))
            .expect("the existing keyed lease reopens");
        assert_eq!(
            super::super::accounting::physical_usage(store.root())
                .expect("the stable inventory reads"),
            admitted
        );
        store
            .retire_fixture()
            .expect("the keyed-lock fixture retires");
    }

    #[test]
    fn existing_bootstrap_never_recreates_a_missing_fixed_control() {
        let store = fixture("missing-fixed");
        let root = store.root().to_path_buf();
        drop(store);
        let missing = root.join("locks/heavy.lock");
        fs::remove_file(&missing).expect("the fixed control is removed by the fixture");

        assert!(crate::store::Store::open(&root, [0x93; 32]).is_err());
        assert!(!missing.exists());
        fs::remove_dir_all(root).expect("the missing-control fixture retires");
    }

    #[test]
    fn inventory_rejects_unknown_control_shape_instead_of_charging_it() {
        let store = fixture("unknown");
        fs::create_dir(store.root().join("unknown")).expect("the unknown control path exists");
        assert!(matches!(
            super::super::accounting::physical_usage(store.root()),
            Err(Error::CleanupRequired(
                "typed store physical inventory found a path outside the closed grammar"
            ))
        ));
        store
            .retire_fixture()
            .expect("the unknown-control fixture retires");
    }

    #[test]
    fn closed_plan_grammar_covers_only_each_canonical_control_mutation() {
        let digest = "11".repeat(32);
        assert!(valid_plan(
            Path::new("actions/runner/0002"),
            Operation::Directory
        ));
        assert!(valid_plan(
            Path::new(&format!("executions/{digest}")),
            Operation::Directory
        ));
        assert!(valid_plan(
            Path::new(&format!("locks/actions/{digest}.lock")),
            Operation::File
        ));
        assert!(valid_plan(
            Path::new(&format!("indexes/runner/{digest}/{digest}")),
            Operation::Publication
        ));
        assert!(valid_plan(
            Path::new(&format!("pins/live/{digest}")),
            Operation::Publication
        ));
        assert!(valid_plan(
            Path::new(&format!("ledgers/projects/{digest}")),
            Operation::Replacement
        ));
        assert!(valid_plan(
            Path::new(&format!("actions/runner/0002/{digest}")),
            Operation::Retirement
        ));
        assert!(valid_plan(
            Path::new(&format!("actions/runner/0001/{digest}")),
            Operation::Retirement
        ));
        assert!(valid_plan(
            Path::new(&format!("executions/{digest}/{digest}")),
            Operation::Retirement
        ));
        assert!(!valid_plan(Path::new("unknown"), Operation::Directory));
        assert!(!valid_plan(
            Path::new(&format!("locks/actions/{digest}")),
            Operation::File
        ));
        assert!(!valid_plan(
            Path::new(&format!("executions/{digest}/{digest}")),
            Operation::Directory
        ));
    }

    #[test]
    fn project_ledger_inventory_admits_exact_installation_cap_only() {
        let mut ledgers = 0;
        for _ in 0..super::super::pins::MAX_PROJECT_LEDGERS {
            admit_project_ledger(&mut ledgers).expect("the exact project-ledger cap admits");
        }
        assert!(matches!(
            admit_project_ledger(&mut ledgers),
            Err(Error::Limit {
                resource: "typed store installation project ledgers",
                limit: super::super::pins::MAX_PROJECT_LEDGERS,
                actual,
            }) if actual == super::super::pins::MAX_PROJECT_LEDGERS + 1
        ));
    }
}

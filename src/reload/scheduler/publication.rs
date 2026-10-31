//! Exact source-input publication staged and retired around host admission.

use std::collections::BTreeSet;
use std::fmt;
use std::ops::Deref;
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard, Weak};

use crate::reload::source;

/// One shared exact-input publication with current and in-flight stages.
pub struct Slot {
    identity: Stamp,
    state: RwLock<State>,
}

/// A borrowed view whose relevance is the union of current and staged inputs.
pub struct Read<'a>(RwLockReadGuard<'a, State>);

/// Observation coverage registered before compilation or source validation.
/// This is not evidence of current native contents or source membership.
#[must_use = "registered observation must be bound to a snapshot or withdrawn"]
pub struct Observed(Option<Observation>);

/// A supplied observation omitted an exact Rawr input identity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct IncompleteObservation;

impl fmt::Display for IncompleteObservation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("observation omits an exact Rawr snapshot input")
    }
}

impl std::error::Error for IncompleteObservation {}

/// Candidate source evidence registered for relevance.
///
/// ```compile_fail
/// # use rawr::reload::scheduler::publication::Staged;
/// fn duplicate(stage: Staged) {
///     let _other = stage.clone();
/// }
/// ```
#[must_use = "a staged input graph must be retired"]
pub struct Staged(Option<Observation>);

struct State {
    current: Arc<source::Inputs>,
    staged: Vec<Entry>,
}

struct Entry {
    stamp: Stamp,
    inputs: Arc<source::Inputs>,
}

struct Observation {
    slot: Weak<Slot>,
    identity: Stamp,
    stamp: Stamp,
    inputs: Arc<source::Inputs>,
}

#[derive(Clone)]
struct Stamp(Arc<()>);

impl Slot {
    /// Install one initial exact input publication.
    #[must_use]
    pub fn new(inputs: source::Inputs) -> Self {
        Self {
            identity: Stamp::new(),
            state: RwLock::new(State {
                current: Arc::new(inputs),
                staged: Vec::new(),
            }),
        }
    }

    /// Register complete current/staged event relevance before watcher roots
    /// are prepared. Dropping the result withdraws only this registration.
    /// Native freshness must still be validated separately by its owner.
    pub fn register(self: &Arc<Self>, inputs: source::Inputs) -> Observed {
        self.register_shared(Arc::new(inputs))
    }

    /// Register transported identities. An edit that keeps the current
    /// identities (a source's text, not its set) shares the current inputs
    /// instead of deriving their directories again.
    #[cfg(all(feature = "rust-import", unix))]
    pub fn register_decoded(self: &Arc<Self>, decoded: source::observation::Decoded) -> Observed {
        let current = Arc::clone(&read(&self.state).current);
        if decoded.identifies(&current) {
            return self.register_shared(current);
        }
        self.register(decoded.into_inputs())
    }

    fn register_shared(self: &Arc<Self>, inputs: Arc<source::Inputs>) -> Observed {
        let stamp = Stamp::new();
        write(&self.state).staged.push(Entry {
            stamp: stamp.clone(),
            inputs: Arc::clone(&inputs),
        });
        Observed(Some(Observation {
            slot: Arc::downgrade(self),
            identity: self.identity.clone(),
            stamp,
            inputs,
        }))
    }

    fn stage(self: &Arc<Self>, snapshot: source::Snapshot) -> Staged {
        // The exact same input Arc already proves coverage; ordinary Rawr
        // staging neither clones path storage nor needs a second coverage scan.
        self.register_shared(Arc::clone(snapshot.shared_inputs()))
            .bind_exact()
    }

    /// Borrow current and every still-live staged exact input set.
    #[must_use]
    pub fn read(&self) -> Read<'_> {
        Read(read(&self.state))
    }

    fn withdraw(&self, proof: &Observation) -> Option<Entry> {
        if !proof.identity.matches(&self.identity) {
            return None;
        }
        let mut state = write(&self.state);
        let index = state
            .staged
            .iter()
            .position(|entry| entry.stamp.matches(&proof.stamp))?;
        Some(state.staged.swap_remove(index))
    }
}

impl<T> super::Candidate<(T, source::Snapshot)> {
    /// Stage this exact revision's candidate inputs before host preparation.
    #[must_use = "the revision-bearing candidate must be handled"]
    pub fn stage_inputs(self, publication: &Arc<Slot>) -> super::Candidate<(T, Staged)> {
        self.map(|(value, snapshot)| (value, publication.stage(snapshot)))
    }
}

impl Read<'_> {
    /// Return the exact currently installed input publication.
    #[must_use]
    pub fn current(&self) -> &source::Inputs {
        &self.0.current
    }

    /// Return whether current or any live staged input set contains this path.
    #[must_use]
    pub fn contains(&self, path: &std::path::Path) -> bool {
        self.0.current.contains(path)
            || self
                .0
                .staged
                .iter()
                .any(|entry| entry.inputs.contains(path))
    }

    /// Return whether every observing current or staged set gives this exact
    /// path only a Rawr source role. A staged role cannot hide a native input.
    #[must_use]
    pub fn is_source_only(&self, path: &std::path::Path) -> bool {
        let mut source = false;
        for inputs in
            std::iter::once(&self.0.current).chain(self.0.staged.iter().map(|entry| &entry.inputs))
        {
            if inputs.contains(path) {
                if !inputs.is_source_only(path) {
                    return false;
                }
                source = true;
            }
            if inputs
                .paths()
                .iter()
                .any(|input| input.as_path() != path && input.starts_with(path))
                || inputs.trees().iter().any(|tree| tree.starts_with(path))
            {
                return false;
            }
        }
        source
    }

    /// Return whether a path names an exact input or an ancestor removed with it.
    #[must_use]
    pub fn affects(&self, path: &std::path::Path) -> bool {
        self.0.current.contains(path)
            || self
                .0
                .current
                .paths()
                .iter()
                .any(|input| input.starts_with(path))
            || self.0.staged.iter().any(|entry| {
                entry.inputs.contains(path)
                    || entry
                        .inputs
                        .paths()
                        .iter()
                        .any(|input| input.starts_with(path))
            })
    }

    /// Return every exact input path across current and staged publications.
    #[must_use]
    pub fn paths(&self) -> Box<[std::path::PathBuf]> {
        std::iter::once(&self.0.current)
            .chain(self.0.staged.iter().map(|entry| &entry.inputs))
            .flat_map(|inputs| inputs.paths().iter().cloned())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// Return exact input parent paths across current and staged publications.
    /// Parents may not exist yet; native transports choose existing ancestors
    /// when registering or rearming their watches, rather than freezing them
    /// at input discovery time.
    #[must_use]
    pub fn parents(&self) -> Box<[std::path::PathBuf]> {
        std::iter::once(&self.0.current)
            .chain(self.0.staged.iter().map(|entry| &entry.inputs))
            .flat_map(|inputs| inputs.paths().iter())
            .filter_map(|path| path.parent().map(std::path::Path::to_path_buf))
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// Return the stable union of current and staged watcher directories.
    #[must_use]
    pub fn directories(&self) -> Box<[std::path::PathBuf]> {
        let mut directories = BTreeSet::new();
        directories.extend(self.0.current.directories().iter().cloned());
        for entry in &self.0.staged {
            directories.extend(entry.inputs.directories().iter().cloned());
        }
        directories.into_iter().collect()
    }

    /// Return the stable union of current and staged recursive native roots.
    #[must_use]
    pub fn trees(&self) -> Box<[std::path::PathBuf]> {
        let mut trees = BTreeSet::new();
        trees.extend(self.0.current.trees().iter().cloned());
        for entry in &self.0.staged {
            trees.extend(entry.inputs.trees().iter().cloned());
        }
        trees.into_iter().collect()
    }

    /// Return every exact input with more than a Rawr source role across
    /// current and staged publications.
    #[must_use]
    pub fn native_paths(&self) -> Box<[std::path::PathBuf]> {
        std::iter::once(&self.0.current)
            .chain(self.0.staged.iter().map(|entry| &entry.inputs))
            .flat_map(|inputs| {
                inputs
                    .paths()
                    .iter()
                    .filter(|path| !inputs.is_source_only(path))
            })
            .cloned()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    /// Return the number of registered observations, including those not yet
    /// bound to a compiled candidate snapshot.
    #[must_use]
    pub fn staged(&self) -> usize {
        self.0.staged.len()
    }
}

impl Deref for Read<'_> {
    type Target = source::Inputs;

    fn deref(&self) -> &Self::Target {
        self.current()
    }
}

impl Observed {
    /// Borrow the full event filter while registering native watcher roots.
    #[must_use]
    pub fn inputs(&self) -> &source::Inputs {
        &self.observation().inputs
    }

    /// Bind retained Rawr bytes without replacing full observation coverage.
    /// Recursive coverage cannot substitute for an exact Rawr file identity.
    /// This check does not validate imported files, directories or environment.
    pub fn bind(self, snapshot: source::Snapshot) -> Result<Staged, IncompleteObservation> {
        let inputs = self.inputs();
        if !snapshot
            .inputs()
            .paths()
            .iter()
            .all(|path| inputs.paths().binary_search(path).is_ok())
            || !snapshot
                .inputs()
                .trees()
                .iter()
                .all(|path| inputs.trees().binary_search(path).is_ok())
            || !inputs
                .source_paths()
                .iter()
                .all(|path| snapshot.inputs().is_source_only(path))
        {
            return Err(IncompleteObservation);
        }
        Ok(self.bind_exact())
    }

    fn observation(&self) -> &Observation {
        match &self.0 {
            Some(observation) => observation,
            None => unreachable!("registered observation is consumed once"),
        }
    }

    fn bind_exact(mut self) -> Staged {
        let observation = match self.0.take() {
            Some(observation) => observation,
            None => unreachable!("registered observation is consumed once"),
        };
        Staged(Some(observation))
    }
}

impl Staged {
    /// Borrow complete candidate observation coverage.
    #[must_use]
    pub fn inputs(&self) -> &source::Inputs {
        let Some(observation) = self.0.as_ref() else {
            unreachable!("live staged observations retain their payload")
        };
        &observation.inputs
    }
}

impl Drop for Observed {
    fn drop(&mut self) {
        retire(self.0.take());
    }
}

impl Drop for Staged {
    fn drop(&mut self) {
        retire(self.0.take());
    }
}

fn retire(proof: Option<Observation>) {
    let Some(proof) = proof else {
        return;
    };
    let retired = proof.slot.upgrade().and_then(|slot| slot.withdraw(&proof));
    drop(retired);
    drop(proof);
}

fn read<T>(lock: &RwLock<T>) -> RwLockReadGuard<'_, T> {
    match lock.read() {
        Ok(value) => value,
        Err(poisoned) => poisoned.into_inner(),
    }
}

fn write<T>(lock: &RwLock<T>) -> RwLockWriteGuard<'_, T> {
    match lock.write() {
        Ok(value) => value,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl Stamp {
    fn new() -> Self {
        Self(Arc::new(()))
    }

    fn matches(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl fmt::Debug for Slot {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Slot(<private>)")
    }
}

impl fmt::Debug for Read<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Read(<private>)")
    }
}

impl fmt::Debug for Staged {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Staged(<private>)")
    }
}

impl fmt::Debug for Observed {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Observed(<private>)")
    }
}

#[cfg(all(test, feature = "rust-import"))]
mod tests;

//! Accounting-target exceedance records.
//!
//! Accounting budgets are optimization targets, never admission gates. When a
//! charge crosses its target the original account keeps charging, records the
//! exceeding axis with its target and the requested cumulative amount, and keeps
//! a truthful high water. Nothing here constructs an error, sets a refusal latch
//! or alters a product result; the registry is reporting state that a CLI drains
//! into one visible warning per command (one per graph) after it completes.

use std::panic::Location;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// The original account whose target was crossed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Account {
    /// A typed-store graph evaluation account (`node::GraphBudget`).
    Graph,
    /// The sealed Cargo request account.
    Request,
    /// The current stored-input validation account.
    Input,
    /// One Cargo JSON allocation epoch.
    CargoJson,
    /// One canonical extraction transaction (`extract::Allocation`).
    Canonical,
    /// One resolution-universe admission or closure parse.
    Universe,
    /// One Cargo build-plan or product capture.
    Build,
    /// One toolchain discovery or cached toolchain record.
    Toolchain,
    /// One Cargo configuration projection.
    Config,
    /// One symbolic surface construction.
    Surface,
    /// One profile demand or contract evaluation.
    Profile,
    /// One supervisor snapshot.
    Supervisor,
    /// One classification pass.
    Classify,
    /// One bridge generation.
    Bridge,
    /// One runner account.
    Runner,
    /// One membership (manifest/session/document) evaluation.
    Membership,
}

impl Account {
    const fn label(self) -> &'static str {
        match self {
            Self::Graph => "graph",
            Self::Request => "request account",
            Self::Input => "input account",
            Self::CargoJson => "Cargo JSON account",
            Self::Canonical => "canonical account",
            Self::Universe => "universe account",
            Self::Build => "build account",
            Self::Toolchain => "toolchain account",
            Self::Config => "Cargo config account",
            Self::Surface => "surface account",
            Self::Profile => "profile account",
            Self::Supervisor => "supervisor account",
            Self::Classify => "classification account",
            Self::Bridge => "bridge account",
            Self::Runner => "runner account",
            Self::Membership => "membership account",
        }
    }
}

/// Exceedance evidence for one axis of one account.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Axis {
    /// The charged resource, as the account names it.
    pub name: &'static str,
    /// The configured accounting target.
    pub target: u64,
    /// The cumulative amount requested by the first exceeding charge.
    pub first_requested: u64,
    /// The highest cumulative amount charged so far.
    pub high_water: u64,
    /// The first exceeding call site, when the charge path tracks callers.
    pub site: Option<&'static Location<'static>>,
}

/// Fixed exceedance capacity per account. The widest accounts reach about ten
/// distinct axes (the canonical account's seven decoder axes, sidecar bytes and
/// canonical items; the graph account's six per-graph axes); 16 leaves
/// headroom. An axis beyond it is never silently lost: it is counted in
/// `truncated`, which the warning prints.
pub const AXES: usize = 16;

/// Allocation-free per-account exceedance record, in first-exceedance order.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Record {
    axes: [Option<Axis>; AXES],
    /// Exceeding charges on axes beyond the fixed capacity; reported, never
    /// minted into a refusal.
    truncated: u32,
}

impl Record {
    /// An empty record.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            axes: [None; AXES],
            truncated: 0,
        }
    }

    /// Record one exceeding charge. Returns whether this record changed.
    /// Public only so the CLI can inject a record into its reporting tests.
    #[doc(hidden)]
    pub fn exceeded(
        &mut self,
        name: &'static str,
        target: u64,
        requested: u64,
        site: Option<&'static Location<'static>>,
    ) -> bool {
        if self.record(name, target, requested, site) {
            return true;
        }
        if self.axis(name).is_none() {
            self.truncated = self.truncated.saturating_add(1);
            return true;
        }
        false
    }

    /// Record into the fixed slots only; false when unchanged or full.
    fn record(
        &mut self,
        name: &'static str,
        target: u64,
        requested: u64,
        site: Option<&'static Location<'static>>,
    ) -> bool {
        for slot in self.axes.iter_mut() {
            match slot {
                Some(axis) if axis.name == name => {
                    if requested > axis.high_water {
                        axis.high_water = requested;
                        return true;
                    }
                    return false;
                }
                Some(_) => {}
                None => {
                    *slot = Some(Axis {
                        name,
                        target,
                        first_requested: requested,
                        high_water: requested,
                        site,
                    });
                    return true;
                }
            }
        }
        false
    }

    /// Fold another record of this account into this one without resetting
    /// either. Truncated counts only grow, so the larger count is kept; an
    /// axis that no longer fits is counted as truncated.
    pub(crate) fn absorb(&mut self, other: &Self) {
        for axis in other.axes() {
            let fits = self.record(axis.name, axis.target, axis.first_requested, axis.site);
            self.record(axis.name, axis.target, axis.high_water, axis.site);
            if !fits && self.axis(axis.name).is_none() {
                self.truncated = self.truncated.saturating_add(1);
            }
        }
        self.truncated = self.truncated.max(other.truncated);
    }

    /// Whether any axis crossed its target.
    #[must_use]
    pub fn is_exceeded(&self) -> bool {
        self.axes[0].is_some() || self.truncated > 0
    }

    /// Exceeding charges on axes beyond the fixed `AXES` capacity.
    #[must_use]
    pub const fn truncated(&self) -> u32 {
        self.truncated
    }

    /// The first axis that crossed its target.
    #[must_use]
    pub fn first(&self) -> Option<&Axis> {
        self.axes[0].as_ref()
    }

    /// Every recorded axis, in first-exceedance order.
    pub fn axes(&self) -> impl Iterator<Item = &Axis> {
        self.axes.iter().flatten()
    }

    /// The recorded axis with this name.
    #[must_use]
    pub fn axis(&self, name: &str) -> Option<&Axis> {
        self.axes().find(|axis| axis.name == name)
    }
}

/// One account's exceedance as published to the process registry.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Exceedance {
    pub account: Account,
    /// Process-unique account identity; clones of one account share it.
    pub id: u64,
    pub record: Record,
}

/// Fixed registry capacity, independent of every production resource limit.
/// Accounts beyond it are counted, never minted into a refusal, and reported
/// as a final count by `warnings`.
pub const CAPACITY: usize = 256;

/// Distinct account identities beyond `CAPACITY` the registry remembers so a
/// republished dropped account is counted once. Past it the dropped count is
/// a lower bound, which the warning says.
pub const DROPPED_IDENTITIES: usize = 4 * CAPACITY;

/// Fixed-capacity, allocation-free registry of published account records.
///
/// The process registry (`publish`, `drain`, `snapshot`, `published`) is one
/// instance behind a lock; tests build isolated instances so their outcome
/// never depends on what other tests in the process have published.
pub struct Registry {
    slots: [Option<Exceedance>; CAPACITY],
    /// Distinct accounts the full registry could not hold, in drop order.
    dropped: [Option<(Account, u64)>; DROPPED_IDENTITIES],
    /// Number of occupied `dropped` entries.
    dropped_len: usize,
}

impl Registry {
    /// An empty registry.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            slots: [None; CAPACITY],
            dropped: [None; DROPPED_IDENTITIES],
            dropped_len: 0,
        }
    }

    /// Publish or refresh one account's record. Allocation-free; a full
    /// registry counts the dropped account once, however often it republishes,
    /// and never fails the caller.
    ///
    /// A republished account merges monotonically into its slot: every axis is
    /// kept with its first crossing, high waters only rise, and axes are the
    /// union of every publish. Publishes race with later charges on the same
    /// account, so a stale record can never move a high water backwards or
    /// drop an axis a newer publish recorded.
    pub fn publish(&mut self, account: Account, id: u64, record: &Record) {
        let existing = self.slots.iter().position(|slot| {
            slot.as_ref()
                .is_some_and(|slot| slot.account == account && slot.id == id)
        });
        if let Some(index) = existing {
            if let Some(slot) = self.slots[index].as_mut() {
                slot.record.absorb(record);
            }
            return;
        }
        match self.slots.iter().position(Option::is_none) {
            Some(index) => {
                self.slots[index] = Some(Exceedance {
                    account,
                    id,
                    record: *record,
                });
            }
            None => {
                let identity = Some((account, id));
                let known = self.dropped[..self.dropped_len].contains(&identity);
                if !known && self.dropped_len < DROPPED_IDENTITIES {
                    self.dropped[self.dropped_len] = identity;
                    self.dropped_len += 1;
                }
            }
        }
    }

    /// Take every published exceedance, leaving the registry empty, together
    /// with the number of further distinct exceeding accounts the fixed
    /// capacity could not hold (a lower bound once it reaches
    /// `DROPPED_IDENTITIES`).
    pub fn drain(&mut self) -> (Vec<Exceedance>, u64) {
        let drained = self.slots.iter_mut().filter_map(Option::take).collect();
        let dropped = self.dropped();
        self.dropped = [None; DROPPED_IDENTITIES];
        self.dropped_len = 0;
        (drained, dropped)
    }

    /// Read every published exceedance without clearing the registry.
    #[must_use]
    pub fn snapshot(&self) -> Vec<Exceedance> {
        self.slots.iter().flatten().copied().collect()
    }

    /// Read the published exceedance for one account, if any.
    #[must_use]
    pub fn published(&self, account: Account, id: u64) -> Option<Exceedance> {
        self.slots
            .iter()
            .flatten()
            .find(|slot| slot.account == account && slot.id == id)
            .copied()
    }

    /// The number of occupied slots.
    #[must_use]
    pub fn len(&self) -> usize {
        self.slots.iter().flatten().count()
    }

    /// Whether no account is published.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.slots.iter().all(Option::is_none)
    }

    /// Distinct exceeding accounts the fixed capacity could not hold since the
    /// last drain.
    #[must_use]
    pub const fn dropped(&self) -> u64 {
        self.dropped_len as u64
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry::new());

static NEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Mint one process-unique account identity. Every account that publishes to
/// the registry, in this crate or a dependent one (the runner account), draws
/// from this one space, so two unrelated accounts never share a key.
#[must_use]
pub fn next_id() -> u64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

/// Publish or refresh one account's record in the process registry; see
/// [`Registry::publish`]. Public only so the CLI can inject a record into its
/// own reporting tests.
#[doc(hidden)]
pub fn publish(account: Account, id: u64, record: &Record) {
    if let Ok(mut registry) = REGISTRY.lock() {
        registry.publish(account, id, record);
    }
}

/// Take every published exceedance from the process registry, leaving it
/// empty, together with the number of further exceeding accounts the fixed
/// capacity could not hold.
#[must_use]
pub fn drain() -> (Vec<Exceedance>, u64) {
    REGISTRY
        .lock()
        .map(|mut registry| registry.drain())
        .unwrap_or_default()
}

/// Read every exceedance published to the process registry without clearing it.
#[must_use]
pub fn snapshot() -> Vec<Exceedance> {
    REGISTRY
        .lock()
        .map(|registry| registry.snapshot())
        .unwrap_or_default()
}

/// Read the process registry's published exceedance for one account, if any.
#[must_use]
pub fn published(account: Account, id: u64) -> Option<Exceedance> {
    REGISTRY
        .lock()
        .ok()
        .and_then(|registry| registry.published(account, id))
}

/// One account's axes: name, high water, target and first exceeding site, plus
/// the count of exceeding charges on axes beyond the record's capacity.
fn axes_text(record: &Record) -> String {
    let mut line = String::new();
    for (index, axis) in record.axes().enumerate() {
        if index > 0 {
            line.push(';');
        }
        line.push_str(&format!(
            " {} reached {} (target {})",
            axis.name, axis.high_water, axis.target
        ));
        if let Some(site) = axis.site {
            line.push_str(&format!(" first at {}:{}", site.file(), site.line()));
        }
    }
    if record.truncated > 0 {
        line.push_str(&format!(
            "; {} further exceeding charge(s) on axes beyond the {AXES}-axis record capacity",
            record.truncated
        ));
    }
    line
}

/// The closing remark for accounts the registry's fixed capacity dropped.
fn dropped_text(dropped: u64) -> String {
    let bound = if dropped >= DROPPED_IDENTITIES as u64 {
        "at least "
    } else {
        ""
    };
    format!(
        "{bound}{dropped} further account(s) exceeded an accounting target beyond the \
         {CAPACITY}-account report capacity"
    )
}

/// Render exactly one warning line per account that crossed any target, plus
/// one closing line when `dropped` further accounts exceeded the registry.
/// Per-account evidence lines; the CLI emits them as one block with `report`.
#[must_use]
pub fn warnings(exceedances: &[Exceedance], dropped: u64) -> Vec<String> {
    let mut lines: Vec<String> = exceedances
        .iter()
        .filter(|exceedance| exceedance.record.is_exceeded())
        .map(|exceedance| {
            format!(
                "WARNING: accounting target exceeded in {} #{}:{}",
                exceedance.account.label(),
                exceedance.id,
                axes_text(&exceedance.record)
            )
        })
        .collect();
    if dropped > 0 {
        lines.push(format!("WARNING: {}", dropped_text(dropped)));
    }
    lines
}

/// Render one command's (one graph's) exceedances as exactly one warning
/// message (one visible warning per graph): a single `WARNING:` header
/// followed by one indented line per exceeding account naming each axis, its
/// target, its high water and its first exceeding site. `None` when nothing
/// crossed a target.
#[must_use]
pub fn report(exceedances: &[Exceedance], dropped: u64) -> Option<String> {
    let exceeding: Vec<&Exceedance> = exceedances
        .iter()
        .filter(|exceedance| exceedance.record.is_exceeded())
        .collect();
    if exceeding.is_empty() && dropped == 0 {
        return None;
    }
    let accounts = exceeding.len() as u64 + dropped;
    let mut message = format!(
        "WARNING: accounting targets exceeded in {accounts} account(s); the result is unchanged:"
    );
    for exceedance in exceeding {
        message.push_str(&format!(
            "\n  {} #{}:{}",
            exceedance.account.label(),
            exceedance.id,
            axes_text(&exceedance.record)
        ));
    }
    if dropped > 0 {
        message.push_str(&format!("\n  {}", dropped_text(dropped)));
    }
    Some(message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn record_keeps_first_axis_and_truthful_high_water() {
        let mut record = Record::new();
        assert!(!record.is_exceeded());
        assert!(record.exceeded("paths", 10, 11, None));
        assert!(record.exceeded("paths", 10, 15, None));
        assert!(!record.exceeded("paths", 10, 12, None));
        assert!(record.exceeded("decoded", 100, 101, None));
        let first = record.first().unwrap();
        assert_eq!(
            (
                first.name,
                first.target,
                first.first_requested,
                first.high_water
            ),
            ("paths", 10, 11, 15)
        );
        assert_eq!(record.axes().count(), 2);
        assert_eq!(record.axis("decoded").unwrap().high_water, 101);
        for index in 0..AXES {
            let name: &'static str = Box::leak(format!("axis-{index}").into_boxed_str());
            record.exceeded(name, 1, 2, None);
        }
        assert_eq!(record.axes().count(), AXES);
        assert!(record.truncated > 0);
    }

    /// The ninth distinct axis (the old capacity) is kept, and an axis beyond
    /// the 16-axis capacity is counted and printed rather than silently lost.
    #[test]
    fn axes_beyond_the_old_capacity_are_kept_and_beyond_the_new_one_counted() {
        const { assert!(AXES >= 16) };
        let names: Vec<&'static str> = (0..=AXES)
            .map(|index| &*Box::leak(format!("axis-{index}").into_boxed_str()))
            .collect();
        let mut record = Record::new();
        for (index, name) in names.iter().enumerate().take(9) {
            assert!(record.exceeded(name, 1, 2 + index as u64, None));
        }
        assert_eq!(record.axes().count(), 9, "a ninth axis is not lost");
        assert_eq!(record.axis("axis-8").unwrap().high_water, 10);
        for name in &names[9..AXES] {
            assert!(record.exceeded(name, 1, 2, None));
        }
        assert_eq!((record.axes().count(), record.truncated()), (AXES, 0));
        // One more distinct axis is counted, and the change is reported.
        assert!(record.exceeded(names[AXES], 1, 2, None));
        assert_eq!(record.truncated(), 1);
        // A charge on a recorded axis below its high water is no change.
        assert!(!record.exceeded(names[0], 1, 2, None));
        assert_eq!(record.truncated(), 1);
        let line = &warnings(
            &[Exceedance {
                account: Account::Canonical,
                id: 5,
                record,
            }],
            0,
        )[0];
        assert!(line.contains("axis-15 reached 2 (target 1)"), "{line}");
        assert!(
            line.contains(
                "1 further exceeding charge(s) on axes beyond the 16-axis record capacity"
            ),
            "{line}"
        );
        // A monotonic merge of the same account keeps the truncated count.
        let mut merged = Record::new();
        merged.absorb(&record);
        assert_eq!(merged, record);
        let mut registry = Registry::new();
        registry.publish(Account::Canonical, 5, &record);
        registry.publish(Account::Canonical, 5, &Record::new());
        assert_eq!(
            registry
                .published(Account::Canonical, 5)
                .unwrap()
                .record
                .truncated(),
            1
        );
    }

    const PATHS: &str = "typed store candidate path checks per graph";
    const DECODED: &str = "typed store decoded candidate bytes per graph";

    fn paths(high_water: u64) -> Record {
        let mut record = Record::new();
        record.exceeded(PATHS, 65_536, 65_540, None);
        record.exceeded(PATHS, 65_536, high_water, None);
        record
    }

    #[test]
    fn empty_registry_publishes_nothing() {
        let mut registry = Registry::new();
        assert!(registry.is_empty());
        assert_eq!(registry.len(), 0);
        assert_eq!(registry.dropped(), 0);
        assert!(registry.snapshot().is_empty());
        assert_eq!(registry.published(Account::Graph, 1), None);
        assert_eq!(registry.drain(), (Vec::new(), 0));
        assert!(warnings(&registry.snapshot(), 0).is_empty());
    }

    #[test]
    fn one_slot_per_account_merges_republished_records_monotonically() {
        let mut registry = Registry::new();
        // Thread A captures the record at 65,540 and is descheduled before it
        // publishes; thread B charges to 70,000, crosses the decode target too,
        // and publishes first. A's stale publish then arrives.
        let stale = paths(65_540);
        let mut newer = paths(70_000);
        newer.exceeded(DECODED, 33_554_432, 33_554_433, None);
        registry.publish(Account::Graph, 7, &newer);
        registry.publish(Account::Graph, 7, &stale);
        assert_eq!(registry.len(), 1, "one slot per account");
        let merged = registry.published(Account::Graph, 7).unwrap().record;
        assert_eq!(merged, newer, "a stale publish moves nothing backwards");
        let axis = merged.axis(PATHS).unwrap();
        assert_eq!(
            (axis.first_requested, axis.high_water),
            (65_540, 70_000),
            "first crossing kept, maximum high water taken"
        );
        assert_eq!(merged.axes().count(), 2, "no axis dropped");
        // The reverse interleaving converges on the same slot contents, and a
        // stale record carrying an axis the newer one lacks is unioned in.
        let mut reversed = Registry::new();
        reversed.publish(Account::Graph, 7, &stale);
        reversed.publish(Account::Graph, 7, &newer);
        assert_eq!(
            reversed.published(Account::Graph, 7),
            registry.published(Account::Graph, 7)
        );
        let mut other_axis = Record::new();
        other_axis.exceeded("typed store hashed candidate bytes per graph", 10, 11, None);
        registry.publish(Account::Graph, 7, &other_axis);
        let merged = registry.published(Account::Graph, 7).unwrap().record;
        assert_eq!(merged.axes().count(), 3);
        assert_eq!(
            merged.first().unwrap().name,
            PATHS,
            "first-exceedance order kept"
        );
        assert_eq!(merged.axis(PATHS).unwrap().high_water, 70_000);
        // Distinct identities and distinct accounts take their own slots.
        registry.publish(Account::Graph, 8, &stale);
        registry.publish(Account::Request, 7, &stale);
        assert_eq!(registry.len(), 3);
        assert_eq!(registry.dropped(), 0);
        let (drained, dropped) = registry.drain();
        assert_eq!((drained.len(), dropped), (3, 0));
        assert!(registry.is_empty());
        assert_eq!(registry.published(Account::Graph, 7), None);
    }

    #[test]
    fn full_registry_counts_new_accounts_and_still_merges_published_ones() {
        let mut registry = Registry::new();
        for id in 0..CAPACITY as u64 {
            registry.publish(Account::Graph, id, &paths(65_540));
        }
        assert_eq!(registry.len(), CAPACITY);
        assert_eq!(registry.dropped(), 0);
        registry.publish(Account::Request, 1, &paths(65_540));
        registry.publish(Account::Graph, CAPACITY as u64, &paths(65_540));
        assert_eq!(
            registry.dropped(),
            2,
            "accounts beyond capacity are counted"
        );
        assert_eq!(registry.published(Account::Request, 1), None);
        // A published account keeps merging while the registry is full.
        registry.publish(Account::Graph, 3, &paths(70_000));
        assert_eq!(
            registry
                .published(Account::Graph, 3)
                .unwrap()
                .record
                .axis(PATHS)
                .unwrap()
                .high_water,
            70_000
        );
        assert_eq!(registry.dropped(), 2);
        // A dropped account that keeps charging republishes on every crossing
        // charge; it is still one account.
        for high_water in 65_541..65_600 {
            registry.publish(Account::Request, 1, &paths(high_water));
            registry.publish(Account::Graph, CAPACITY as u64, &paths(high_water));
        }
        assert_eq!(registry.dropped(), 2, "distinct accounts, not publishes");
        // The same identity under another account kind is another account.
        registry.publish(Account::Input, 1, &paths(65_540));
        assert_eq!(registry.dropped(), 3);
        registry.drain();
        for id in 0..CAPACITY as u64 {
            registry.publish(Account::Graph, id, &paths(65_540));
        }
        registry.publish(Account::Request, 1, &paths(65_540));
        registry.publish(Account::Graph, CAPACITY as u64, &paths(65_540));
        let lines = warnings(&registry.snapshot(), registry.dropped());
        assert_eq!(lines.len(), CAPACITY + 1);
        assert!(lines[CAPACITY].contains("2 further account(s)"));
        assert!(lines[CAPACITY].contains(&format!("{CAPACITY}-account report capacity")));
        let block = report(&registry.snapshot(), registry.dropped()).unwrap();
        assert_eq!(block.matches("WARNING").count(), 1, "one message");
        assert!(block.contains(&format!("in {} account(s)", CAPACITY + 2)));
        assert!(block.ends_with(&format!(
            "\n  2 further account(s) exceeded an accounting target beyond the \
             {CAPACITY}-account report capacity"
        )));
        let (drained, dropped) = registry.drain();
        assert_eq!((drained.len(), dropped), (CAPACITY, 2));
        assert!(registry.is_empty());
        assert_eq!(registry.dropped(), 0);
        registry.publish(Account::Request, 1, &paths(65_540));
        assert_eq!(registry.len(), 1, "a drained registry accepts new accounts");
    }

    #[test]
    fn warnings_render_one_line_per_exceeding_account() {
        let mut registry = Registry::new();
        let mut graph = paths(70_000);
        graph.exceeded(DECODED, 33_554_432, 33_554_433, None);
        registry.publish(Account::Graph, 11, &graph);
        let mut request = Record::new();
        request.exceeded("request retained bytes", 1_048_576, 1_048_577, None);
        registry.publish(Account::Request, 12, &request);
        registry.publish(Account::Input, 13, &Record::new());
        let published = registry.snapshot();
        assert_eq!(published.len(), 3);
        let lines = warnings(&published, 0);
        assert_eq!(
            lines.len(),
            2,
            "an account without a crossing renders nothing"
        );
        assert_eq!(warnings(&published, 3).len(), 3);
        assert!(warnings(&published, 3)[2].contains("3 further account(s)"));
        assert!(lines[0].starts_with("WARNING: accounting target exceeded in graph #11:"));
        assert!(lines[0]
            .contains("typed store candidate path checks per graph reached 70000 (target 65536);"));
        assert!(lines[0].contains(
            "typed store decoded candidate bytes per graph reached 33554433 (target 33554432)"
        ));
        assert_eq!(
            lines[1],
            "WARNING: accounting target exceeded in request account #12: request retained bytes \
             reached 1048577 (target 1048576)"
        );
    }

    /// One command (one graph) reports every exceeding account in one
    /// warning message, naming each axis with its target, high water and
    /// first site; nothing exceeded reports nothing.
    #[test]
    fn report_renders_one_message_for_every_exceeding_account() {
        assert_eq!(report(&[], 0), None);
        assert_eq!(
            report(
                &[Exceedance {
                    account: Account::Input,
                    id: 1,
                    record: Record::new(),
                }],
                0
            ),
            None
        );
        let site = Location::caller();
        let mut graph = paths(70_000);
        graph.exceeded(DECODED, 33_554_432, 33_554_433, Some(site));
        let mut request = Record::new();
        request.exceeded("request retained bytes", 1_048_576, 1_048_577, None);
        let exceedances = [
            Exceedance {
                account: Account::Graph,
                id: 11,
                record: graph,
            },
            Exceedance {
                account: Account::Request,
                id: 12,
                record: request,
            },
            Exceedance {
                account: Account::Input,
                id: 13,
                record: Record::new(),
            },
        ];
        let message = report(&exceedances, 0).unwrap();
        assert_eq!(message.matches("WARNING").count(), 1);
        assert_eq!(
            message,
            format!(
                "WARNING: accounting targets exceeded in 2 account(s); the result is unchanged:\n  \
                 graph #11: typed store candidate path checks per graph reached 70000 (target \
                 65536); typed store decoded candidate bytes per graph reached 33554433 (target \
                 33554432) first at {}:{}\n  request account #12: request retained bytes reached \
                 1048577 (target 1048576)",
                site.file(),
                site.line()
            )
        );
    }

    /// The process registry is one `Registry` instance. Its only reason to hold
    /// no slot for a published account is being full, which other tests in this
    /// process may have caused; the deterministic behavior is proven above on
    /// isolated instances.
    #[test]
    fn process_registry_publishes_into_one_shared_instance() {
        let id = next_id();
        assert_ne!(id, next_id());
        let record = paths(70_000);
        publish(Account::Graph, id, &record);
        match published(Account::Graph, id) {
            Some(mine) => {
                assert_eq!(mine.record, record);
                assert!(snapshot().contains(&mine));
            }
            None => assert_eq!(
                snapshot().len(),
                CAPACITY,
                "only a full process registry holds no slot for a published account"
            ),
        }
    }
}

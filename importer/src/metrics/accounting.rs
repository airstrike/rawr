//! Fixed scalar observations from existing production transformations and accounts.
//!
//! These values describe work already performed. They never authorize work, change
//! a limit, or decide whether a product succeeds. Missing observations remain absent.

use serde::{Deserialize, Serialize};

use super::NodeKind;
use crate::protocol::account;

/// A closed byte/item quantity, observed at its actual producer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Scalar {
    ResolutionMetadataBytes,
    ResolutionUnitBytes,
    ResolutionBuildBytes,
    ResolutionInputBytes,
    ResolutionNamespaceBytes,
    ResolutionRecordBytes,
    ApiCanonicalBytes,
    ApiSelectedBytes,
    ApiTransactionItems,
    ApiTransactionBytes,
    NodePayloadBytes(NodeKind),
    ProcessElapsedNanoseconds,
    ProcessStdoutBytes,
    ProcessStderrBytes,
    CargoRequestItems,
    CargoRequestBytes,
    OwnerInputItems,
    OwnerInputBytes,
    CargoCurrentItems,
    CargoCurrentBytes,
    /// Bytes in retained strings, rather than the number of strings.
    CargoCurrentStrings,
    CargoCurrentContainers,
    CargoPeakItems,
    CargoPeakBytes,
    CargoPeakStrings,
    CargoPeakContainers,
    /// Original `actual` at this stage; units/resource remain in the product error.
    OwnerRefusedRequired(OwnerStage),
    /// Original `limit` paired with that error, not a stand-alone resource proof.
    OwnerRefusedLimit(OwnerStage),
    /// One observation with value 1 for every error returned by the named stage.
    OwnerRefusals(OwnerStage),
    LedgerReadBytes,
    /// Every actual encoding, including canonical re-encodings during validation.
    LedgerEncodedBytes,
    /// Reached control charge; a failed walk may report a partial inventory.
    ControlInventoryBytes,
    ControlInventoryMembers,
    /// Existing measurement charge, including a future allocation record when requested.
    BundleMeasuredBytes,
    BundleMeasuredMembers,
    RecoveryMetadataBytes,
    /// Cumulative deletion admission request, not proof that deletion completed.
    RecoveryDeletionBytes,
    RecoveryDeletionMembers,
    RecoveryBundles,
    RecoveryTemporaries,
    ApiRetainedItems,
    ApiRetainedBytes,
    ClassifiedItems,
    ClassifiedBytes,
    /// Maximum scratch bytes admitted by the original classification context.
    ClassifiedScratchBytes,
    /// Returned failures of the original semantic transformation, including non-limit errors.
    SemanticRefusals(SemanticStage),
    /// Original `actual`; resource and units remain in the unchanged product error.
    SemanticRefusedRequired(SemanticStage),
    /// Original `limit` paired with the refused request.
    SemanticRefusedLimit(SemanticStage),
    NativeInventoryItems,
    NativeInventoryBytes,
    ProjectLedgerReadBytes,
    ProjectLedgerEncodedBytes,
    PinRolesRequested,
    PinRolesCommitted,
    PinRolesRollbackRequested,
}

/// Closed owner operations whose ordinary returned errors can be observed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum OwnerStage {
    InputCapture,
    Preflight,
    Commitment,
    Replay,
    Execute,
}

/// Closed semantic operations, separate from Cargo/input owner operations.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum SemanticStage {
    ApiTransaction,
    ApiRetained,
    Classification,
}

const SCALARS: [Scalar; 83] = [
    Scalar::ResolutionMetadataBytes,
    Scalar::ResolutionUnitBytes,
    Scalar::ResolutionBuildBytes,
    Scalar::ResolutionInputBytes,
    Scalar::ResolutionNamespaceBytes,
    Scalar::ResolutionRecordBytes,
    Scalar::ApiCanonicalBytes,
    Scalar::ApiSelectedBytes,
    Scalar::ApiTransactionItems,
    Scalar::ApiTransactionBytes,
    Scalar::NodePayloadBytes(NodeKind::Toolchain),
    Scalar::NodePayloadBytes(NodeKind::Resolution),
    Scalar::NodePayloadBytes(NodeKind::Api),
    Scalar::NodePayloadBytes(NodeKind::Generic),
    Scalar::NodePayloadBytes(NodeKind::Profile),
    Scalar::NodePayloadBytes(NodeKind::Facade),
    Scalar::NodePayloadBytes(NodeKind::Checked),
    Scalar::NodePayloadBytes(NodeKind::ProfileProof),
    Scalar::NodePayloadBytes(NodeKind::NativeProduct),
    Scalar::NodePayloadBytes(NodeKind::Overlay),
    Scalar::NodePayloadBytes(NodeKind::Runner),
    Scalar::ProcessElapsedNanoseconds,
    Scalar::ProcessStdoutBytes,
    Scalar::ProcessStderrBytes,
    Scalar::CargoRequestItems,
    Scalar::CargoRequestBytes,
    Scalar::OwnerInputItems,
    Scalar::OwnerInputBytes,
    Scalar::CargoCurrentItems,
    Scalar::CargoCurrentBytes,
    Scalar::CargoCurrentStrings,
    Scalar::CargoCurrentContainers,
    Scalar::CargoPeakItems,
    Scalar::CargoPeakBytes,
    Scalar::CargoPeakStrings,
    Scalar::CargoPeakContainers,
    Scalar::OwnerRefusedRequired(OwnerStage::InputCapture),
    Scalar::OwnerRefusedRequired(OwnerStage::Preflight),
    Scalar::OwnerRefusedRequired(OwnerStage::Commitment),
    Scalar::OwnerRefusedRequired(OwnerStage::Replay),
    Scalar::OwnerRefusedRequired(OwnerStage::Execute),
    Scalar::OwnerRefusedLimit(OwnerStage::InputCapture),
    Scalar::OwnerRefusedLimit(OwnerStage::Preflight),
    Scalar::OwnerRefusedLimit(OwnerStage::Commitment),
    Scalar::OwnerRefusedLimit(OwnerStage::Replay),
    Scalar::OwnerRefusedLimit(OwnerStage::Execute),
    Scalar::OwnerRefusals(OwnerStage::InputCapture),
    Scalar::OwnerRefusals(OwnerStage::Preflight),
    Scalar::OwnerRefusals(OwnerStage::Commitment),
    Scalar::OwnerRefusals(OwnerStage::Replay),
    Scalar::OwnerRefusals(OwnerStage::Execute),
    Scalar::LedgerReadBytes,
    Scalar::LedgerEncodedBytes,
    Scalar::ControlInventoryBytes,
    Scalar::ControlInventoryMembers,
    Scalar::BundleMeasuredBytes,
    Scalar::BundleMeasuredMembers,
    Scalar::RecoveryMetadataBytes,
    Scalar::RecoveryDeletionBytes,
    Scalar::RecoveryDeletionMembers,
    Scalar::RecoveryBundles,
    Scalar::RecoveryTemporaries,
    Scalar::ApiRetainedItems,
    Scalar::ApiRetainedBytes,
    Scalar::ClassifiedItems,
    Scalar::ClassifiedBytes,
    Scalar::NativeInventoryItems,
    Scalar::NativeInventoryBytes,
    Scalar::ProjectLedgerReadBytes,
    Scalar::ProjectLedgerEncodedBytes,
    Scalar::PinRolesRequested,
    Scalar::PinRolesCommitted,
    Scalar::PinRolesRollbackRequested,
    Scalar::ClassifiedScratchBytes,
    Scalar::SemanticRefusals(SemanticStage::ApiTransaction),
    Scalar::SemanticRefusals(SemanticStage::ApiRetained),
    Scalar::SemanticRefusals(SemanticStage::Classification),
    Scalar::SemanticRefusedRequired(SemanticStage::ApiTransaction),
    Scalar::SemanticRefusedRequired(SemanticStage::ApiRetained),
    Scalar::SemanticRefusedRequired(SemanticStage::Classification),
    Scalar::SemanticRefusedLimit(SemanticStage::ApiTransaction),
    Scalar::SemanticRefusedLimit(SemanticStage::ApiRetained),
    Scalar::SemanticRefusedLimit(SemanticStage::Classification),
];

/// An axis in the original graph account, in the order used by `Graph::admitted`.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Axis {
    Lookups,
    Comparisons,
    Deep,
    Decoded,
    Paths,
    Hashed,
}

/// The closed operation at which the original account was observed.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stage {
    Charge(Axis),
    Node(NodeKind),
    Admission,
    Watch,
    Resident,
    Completion,
}

/// A cumulative charge that crossed its accounting target. The charge
/// was still admitted to the original account; this is reporting, not refusal.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Exceedance {
    pub axis: Axis,
    /// Attempted cumulative total; absent only when checked arithmetic overflowed.
    pub requested: Option<u64>,
    /// The accounting target the request crossed.
    pub limit: u64,
}

/// One allocation-free observation of the original shared graph account.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Graph {
    /// Observation identity only. Clones of an account retain the same nonzero ID.
    pub account: u64,
    pub stage: Stage,
    /// Lookups, comparisons, deep validations, decoded bytes, paths, hashed bytes.
    pub admitted: [u64; 6],
    pub exceedance: Option<Exceedance>,
}

/// Repeated observations retain their population, range, and checked total.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Range {
    pub count: u64,
    pub min: u64,
    pub max: u64,
    pub total: u64,
}

/// One canonical scalar slot; `None` distinguishes unknown from measured zero.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScalarRow {
    pub scalar: Scalar,
    pub observed: Option<Range>,
}

/// The first observed exceedance, including its original admitted usage and stage.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Exceeded {
    pub stage: Stage,
    pub admitted: [u64; 6],
    #[serde(alias = "refusal")]
    pub exceedance: Exceedance,
}

/// Bounded cumulative evidence for one original account, never a replacement budget.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GraphRow {
    pub account: u64,
    pub observations: u64,
    /// First and last in observation arrival order, which can differ from charge order.
    pub first_stage: Stage,
    pub last_stage: Stage,
    pub high_water: [u64; 6],
    #[serde(alias = "refusals")]
    pub exceedances: u64,
    #[serde(alias = "first_refusal")]
    pub first_exceedance: Option<Exceeded>,
}

/// A sticky recording failure. It cannot replace the product's original error.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Fault {
    ScalarOverflow(Scalar),
    GraphOverflow(u64),
    GraphCapacity,
    InvalidGraph(u64),
    Native {
        runner_pid: u32,
        epoch: u64,
        cause: NativeFault,
    },
}

/// Required telemetry faults affect evidence completeness, never product outcome.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum NativeFault {
    Missing,
    Malformed,
    Mismatched,
    Duplicate,
    Identity,
    Capacity,
    Overflow,
    Inconsistent,
}

/// One original native account observed through an authenticated runner lifetime.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeRow {
    pub runner_pid: u32,
    pub epoch: u64,
    pub observations: u64,
    /// Bit N records an observed required command whose protocol tag is N.
    pub commands: u64,
    /// Maximum simultaneous distinct accounts seen with this row (zero if unreached).
    pub simultaneous: u8,
    /// Last accepted current usage, cumulative peak, and immutable first refusal.
    /// None records an explicitly unreached account, rather than measured zero.
    pub snapshot: Option<account::Snapshot>,
}

/// Fixed original-account slots, including explicit unreached runner lifetimes.
pub const NATIVE_ACCOUNTS: usize = 32;

/// Fixed evidence capacity, independent of all production resource limits.
pub const GRAPH_ACCOUNTS: usize = 32;

/// Closed scalar evidence carried in the existing recorder snapshot and capsule.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    #[serde(
        serialize_with = "serialize_scalars",
        deserialize_with = "deserialize_scalars"
    )]
    pub scalars: [ScalarRow; SCALARS.len()],
    pub graphs: [Option<GraphRow>; GRAPH_ACCOUNTS],
    pub native: [Option<NativeRow>; NATIVE_ACCOUNTS],
    /// A fault freezes this evidence at the last representable observation.
    pub fault: Option<Fault>,
}

impl Default for Evidence {
    fn default() -> Self {
        Self {
            scalars: SCALARS.map(|scalar| ScalarRow {
                scalar,
                observed: None,
            }),
            graphs: [None; GRAPH_ACCOUNTS],
            native: [None; NATIVE_ACCOUNTS],
            fault: None,
        }
    }
}

impl Evidence {
    /// Read an observed range; an unvisited producer is explicitly unknown.
    pub fn scalar(&self, scalar: Scalar) -> Option<Range> {
        self.scalars
            .iter()
            .find(|row| row.scalar == scalar)
            .and_then(|row| row.observed)
    }

    /// Check structure without discarding diagnostic evidence containing a fault.
    pub fn valid(&self) -> bool {
        if self
            .scalars
            .iter()
            .zip(SCALARS)
            .any(|(row, scalar)| row.scalar != scalar || row.observed.is_some_and(|r| !r.valid()))
        {
            return false;
        }
        let mut empty = false;
        for (index, row) in self.graphs.iter().enumerate() {
            let Some(row) = row else {
                empty = true;
                continue;
            };
            if empty
                || !row.valid()
                || self.graphs[..index]
                    .iter()
                    .flatten()
                    .any(|previous| previous.account == row.account)
            {
                return false;
            }
        }
        let mut empty = false;
        for (index, row) in self.native.iter().enumerate() {
            let Some(row) = row else {
                empty = true;
                continue;
            };
            if empty
                || !row.valid()
                || self.native[..index]
                    .iter()
                    .flatten()
                    .any(|previous| previous.key() == row.key())
            {
                return false;
            }
            if row
                .snapshot
                .is_some_and(|snapshot| snapshot.account.is_none())
                && !matches!(self.fault, Some(Fault::Native { runner_pid, epoch, cause: NativeFault::Identity }) if runner_pid == row.runner_pid && epoch == row.epoch)
            {
                return false;
            }
        }
        true
    }

    /// Check that this runner lifetime reported each required command, including
    /// truthful unreached reports. This checks telemetry coverage, not resource fit.
    pub fn native_covers(&self, runner_pid: u32, epoch: u64, required: &[account::Tag]) -> bool {
        if !self.valid() || self.fault.is_some() || runner_pid == 0 || epoch == 0 {
            return false;
        }
        let mut seen = false;
        let mut commands = 0;
        for row in self
            .native
            .iter()
            .flatten()
            .filter(|row| row.runner_pid == runner_pid && row.epoch == epoch)
        {
            seen = true;
            commands |= row.commands;
        }
        seen && required
            .iter()
            .all(|tag| account::required(*tag) && commands & command(*tag) != 0)
    }

    pub(super) fn record_native_fault(&mut self, runner_pid: u32, epoch: u64, cause: NativeFault) {
        self.fault.get_or_insert(Fault::Native {
            runner_pid,
            epoch,
            cause,
        });
    }

    pub(super) fn record_native(
        &mut self,
        runner_pid: u32,
        epoch: u64,
        observation: account::Observation,
        limit: u64,
    ) {
        if self.fault.is_some() {
            return;
        }
        if runner_pid == 0 || epoch == 0 {
            self.record_native_fault(runner_pid, epoch, NativeFault::Identity);
            return;
        }
        if observation.validate().is_err() {
            self.record_native_fault(runner_pid, epoch, NativeFault::Malformed);
            return;
        }
        let simultaneous = observation.accounts.iter().flatten().count() as u8;
        let mut updated = self.native;
        let result = if simultaneous == 0 {
            Self::native_row(
                &mut updated,
                runner_pid,
                epoch,
                observation.request.tag(),
                None,
                0,
                limit,
            )
        } else {
            observation
                .accounts
                .into_iter()
                .flatten()
                .try_for_each(|snapshot| {
                    Self::native_row(
                        &mut updated,
                        runner_pid,
                        epoch,
                        observation.request.tag(),
                        Some(snapshot),
                        simultaneous,
                        limit,
                    )
                })
        };
        match result {
            Ok(()) => self.native = updated,
            Err(cause) => self.record_native_fault(runner_pid, epoch, cause),
        }
        if self.fault.is_none()
            && observation
                .accounts
                .iter()
                .flatten()
                .any(|snapshot| snapshot.account.is_none())
        {
            self.record_native_fault(runner_pid, epoch, NativeFault::Identity);
        }
    }

    fn native_row(
        rows: &mut [Option<NativeRow>; NATIVE_ACCOUNTS],
        runner_pid: u32,
        epoch: u64,
        tag: account::Tag,
        snapshot: Option<account::Snapshot>,
        simultaneous: u8,
        limit: u64,
    ) -> Result<(), NativeFault> {
        let key = (
            runner_pid,
            epoch,
            snapshot.and_then(|snapshot| snapshot.account),
        );
        let index = rows
            .iter()
            .position(|row| row.as_ref().is_some_and(|row| row.key() == key))
            .or_else(|| rows.iter().position(Option::is_none))
            .ok_or(NativeFault::Capacity)?;
        let mut row = rows[index].unwrap_or(NativeRow {
            runner_pid,
            epoch,
            observations: 0,
            commands: 0,
            simultaneous: 0,
            snapshot: None,
        });
        row.observations = row
            .observations
            .checked_add(1)
            .filter(|count| *count <= limit)
            .ok_or(NativeFault::Overflow)?;
        row.commands |= command(tag);
        row.simultaneous = row.simultaneous.max(simultaneous);
        if let Some(mut next) = snapshot {
            if let Some(previous) = row.snapshot {
                if previous.limits != next.limits
                    || previous.first_refusal.is_some()
                        && next.first_refusal.is_some()
                        && previous.first_refusal != next.first_refusal
                {
                    return Err(NativeFault::Inconsistent);
                }
                for (peak, previous) in next.peak.iter_mut().zip(previous.peak) {
                    *peak = (*peak).max(previous);
                }
                next.first_refusal = previous.first_refusal.or(next.first_refusal);
            }
            row.snapshot = Some(next);
        }
        rows[index] = Some(row);
        Ok(())
    }

    pub(super) fn record_scalar(&mut self, scalar: Scalar, value: u64, limit: u64) {
        if self.fault.is_some() {
            return;
        }
        let Some(row) = self.scalars.iter_mut().find(|row| row.scalar == scalar) else {
            self.fault = Some(Fault::ScalarOverflow(scalar));
            return;
        };
        let next = match row.observed {
            None => Some(Range {
                count: 1,
                min: value,
                max: value,
                total: value,
            }),
            Some(range) => range.push(value, limit),
        };
        match next.filter(|range| range.count <= limit) {
            Some(range) => row.observed = Some(range),
            None => self.fault = Some(Fault::ScalarOverflow(scalar)),
        }
    }

    pub(super) fn scalar_overflow(&mut self, scalar: Scalar) {
        self.fault.get_or_insert(Fault::ScalarOverflow(scalar));
    }

    pub(super) fn record_graph(&mut self, graph: Graph, limit: u64) {
        if self.fault.is_some() {
            return;
        }
        if graph.account == 0
            || graph
                .exceedance
                .is_some_and(|exceedance| !exceedance.valid())
        {
            self.fault = Some(Fault::InvalidGraph(graph.account));
            return;
        }
        let existing = self
            .graphs
            .iter()
            .position(|row| row.as_ref().is_some_and(|row| row.account == graph.account));
        let index = existing.or_else(|| self.graphs.iter().position(Option::is_none));
        let Some(index) = index else {
            self.fault = Some(Fault::GraphCapacity);
            return;
        };
        let previous = self.graphs[index];
        let mut row = previous.unwrap_or(GraphRow {
            account: graph.account,
            observations: 0,
            first_stage: graph.stage,
            last_stage: graph.stage,
            high_water: [0; 6],
            exceedances: 0,
            first_exceedance: None,
        });
        let Some(observations) = row.observations.checked_add(1).filter(|n| *n <= limit) else {
            self.fault = Some(Fault::GraphOverflow(graph.account));
            return;
        };
        row.observations = observations;
        row.last_stage = graph.stage;
        for (high_water, admitted) in row.high_water.iter_mut().zip(graph.admitted) {
            *high_water = (*high_water).max(admitted);
        }
        if let Some(exceedance) = graph.exceedance {
            // Exceedances are a subset of the checked observation count.
            row.exceedances += 1;
            row.first_exceedance.get_or_insert(Exceeded {
                stage: graph.stage,
                admitted: graph.admitted,
                exceedance,
            });
        }
        self.graphs[index] = Some(row);
    }
}

impl Range {
    fn push(self, value: u64, limit: u64) -> Option<Self> {
        Some(Self {
            count: self.count.checked_add(1).filter(|count| *count <= limit)?,
            min: self.min.min(value),
            max: self.max.max(value),
            total: self.total.checked_add(value)?,
        })
    }

    fn valid(self) -> bool {
        self.count > 0
            && self.min <= self.max
            && u128::from(self.min) * u128::from(self.count - 1) + u128::from(self.max)
                <= u128::from(self.total)
            && u128::from(self.max) * u128::from(self.count - 1) + u128::from(self.min)
                >= u128::from(self.total)
            && (self.count != 1 || self.min == self.max && self.total == self.min)
    }
}

impl GraphRow {
    fn valid(&self) -> bool {
        self.account != 0
            && self.observations > 0
            && self.exceedances <= self.observations
            && (self.exceedances > 0) == self.first_exceedance.is_some()
            && self.first_exceedance.is_none_or(|exceeded| {
                exceeded.exceedance.valid()
                    && exceeded
                        .admitted
                        .iter()
                        .zip(self.high_water)
                        .all(|(a, h)| *a <= h)
            })
    }
}

impl Exceedance {
    /// A recorded exceedance must name a request above its target, or an
    /// arithmetic overflow whose requested total could not be represented.
    fn valid(self) -> bool {
        self.requested.is_none_or(|amount| amount > self.limit)
    }
}

impl NativeRow {
    fn key(&self) -> (u32, u64, Option<std::num::NonZeroU64>) {
        (
            self.runner_pid,
            self.epoch,
            self.snapshot.and_then(|snapshot| snapshot.account),
        )
    }

    fn valid(&self) -> bool {
        let commands = (1_u8..=45)
            .filter_map(|tag| account::Tag::try_from(tag).ok())
            .filter(|tag| account::required(*tag))
            .fold(0, |bits, tag| bits | command(tag));
        self.runner_pid != 0
            && self.epoch != 0
            && self.observations > 0
            && self.commands != 0
            && self.commands & !commands == 0
            && match self.snapshot {
                None => self.simultaneous == 0,
                Some(snapshot) => {
                    (1..=2).contains(&self.simultaneous) && snapshot.validate().is_ok()
                }
            }
    }
}

fn command(tag: account::Tag) -> u64 {
    1_u64 << tag as u8
}

fn serialize_scalars<S>(
    scalars: &[ScalarRow; SCALARS.len()],
    serializer: S,
) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    scalars.as_slice().serialize(serializer)
}

fn deserialize_scalars<'de, D>(deserializer: D) -> Result<[ScalarRow; SCALARS.len()], D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct Visitor;

    impl<'de> serde::de::Visitor<'de> for Visitor {
        type Value = [ScalarRow; SCALARS.len()];

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(formatter, "exactly {} closed scalar rows", SCALARS.len())
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            let mut rows = SCALARS.map(|scalar| ScalarRow {
                scalar,
                observed: None,
            });
            for (index, row) in rows.iter_mut().enumerate() {
                *row = sequence
                    .next_element()?
                    .ok_or_else(|| serde::de::Error::invalid_length(index, &self))?;
            }
            if sequence.next_element::<serde::de::IgnoredAny>()?.is_some() {
                return Err(serde::de::Error::invalid_length(SCALARS.len() + 1, &self));
            }
            Ok(rows)
        }
    }

    deserializer.deserialize_tuple(SCALARS.len(), Visitor)
}

#[cfg(test)]
mod tests;

//! Fixed scalar observations from existing production transformations and accounts.
//!
//! These values describe work already performed. They never authorize work, change
//! a limit, or decide whether a product succeeds. Missing observations remain absent.

use serde::{Deserialize, Serialize};

use super::NodeKind;

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

/// A sticky recording failure. It cannot replace the product's original error.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Fault {
    ScalarOverflow(Scalar),
}

/// Closed scalar evidence carried in the existing recorder snapshot and capsule.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Evidence {
    #[serde(
        serialize_with = "serialize_scalars",
        deserialize_with = "deserialize_scalars"
    )]
    pub scalars: [ScalarRow; SCALARS.len()],
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
        true
    }

    #[cfg(any(test, feature = "test-fixtures"))]
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

    #[cfg(any(test, feature = "test-fixtures"))]
    pub(super) fn scalar_overflow(&mut self, scalar: Scalar) {
        self.fault.get_or_insert(Fault::ScalarOverflow(scalar));
    }
}

impl Range {
    #[cfg(any(test, feature = "test-fixtures"))]
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

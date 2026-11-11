//! The identity of a runner's carrier, its generated adapters, that a
//! stored structural adapter's definition holds: written as nothing and
//! read back as the carrier of the runner that reads it (`super::Known`), as
//! another runner's build may hold it (#1725). A kept lowering is reused
//! only while the reading runner admitted each adapter it calls
//! (`host::Registry::admits_structural`), so a read names an adapter the
//! reading carrier holds.

pub(crate) fn serialize<S: serde::Serializer>(
    _identity: &[u8; 32],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    serializer.serialize_unit()
}

pub(crate) fn deserialize<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<[u8; 32], D::Error> {
    <() as serde::Deserialize>::deserialize(deserializer)?;
    super::known(|known| known.carrier)
        .ok_or_else(|| serde::de::Error::custom("no runner's carrier reads this adapter"))
}

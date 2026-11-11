//! A facade's fingerprint a stored value holds, written as the facade's
//! root and read back as the fingerprint of the facade of that root this
//! process installed (`super::Known`), as another runner's build may hold
//! it (#1725). A kept lowering's key holds what it read of each facade, so
//! a read under the same key reads a facade that holds the same of it. A
//! root this process didn't install fails the read, and what holds it is
//! made again.

use crate::host::facade::Fingerprint;

impl serde::Serialize for Fingerprint {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.root())
    }
}

impl<'de> serde::Deserialize<'de> for Fingerprint {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let root = <String as serde::Deserialize>::deserialize(deserializer)?;
        super::known(|known| known.facades.get(root.as_str()).cloned()).ok_or_else(|| {
            serde::de::Error::custom(format_args!("`{root}` is no facade this process installed"))
        })
    }
}

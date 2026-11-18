//! An outcome with each plan it names in place: all a facade reads of it
//! (#1618). Its digest names what a facade generates for the outcome, so a
//! generation of the same digest is the one kept.

use serde::ser::{SerializeSeq as _, SerializeStruct as _};
use serde::{Serialize, Serializer};
use sha2::{Digest as _, Sha256};

use super::{ImplementationRecord, Outcome};

/// An outcome as a facade reads it.
struct Resolved<'a>(Outcome<'a>);

/// An outcome's implementation ledgers as a facade reads them.
struct Implementations<'a> {
    records: &'a [ImplementationRecord],
    outcome: Outcome<'a>,
}

impl Serialize for Resolved<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let record = self.0.record;
        let mut state = serializer.serialize_struct("Resolved", 9)?;
        state.serialize_field("export", &record.export)?;
        state.serialize_field("identity", &record.identity)?;
        state.serialize_field("name", &record.name)?;
        state.serialize_field("origin", &record.origin)?;
        state.serialize_field("representability", self.0.representability)?;
        state.serialize_field(
            "implementations",
            &Implementations {
                records: &record.implementations,
                outcome: self.0,
            },
        )?;
        state.serialize_field("variant", &record.variant)?;
        state.serialize_field("member", &record.member)?;
        state.serialize_field("outputs", &record.outputs)?;
        state.end()
    }
}

impl Serialize for Implementations<'_> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(Some(self.records.len()))?;
        for implementation in self.outcome.implementations() {
            sequence.serialize_element(&(
                implementation.identity(),
                implementation.span(),
                implementation
                    .associated()
                    .map(Resolved)
                    .collect::<Vec<_>>(),
            ))?;
        }
        sequence.end()
    }
}

impl Outcome<'_> {
    /// The digest of the outcome with each plan it names in place.
    pub(crate) fn resolved_digest(&self) -> [u8; 32] {
        let mut digest = Sha256::new();
        serde_json::to_writer(Writer(&mut digest), &Resolved(*self))
            .expect("an outcome encodes, as the classification's canonical bytes encode it");
        digest.finalize().into()
    }
}

/// Writes JSON straight into a digest.
struct Writer<'a>(&'a mut Sha256);

impl std::io::Write for Writer<'_> {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.update(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

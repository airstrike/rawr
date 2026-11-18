//! The crates a shared table names (#1128).
//!
//! A table names a crate wherever one of its records carries the crate's
//! name and stable id together: a definition's compiler authority, a
//! crate the walk registered, a re-export's hop. A replay of the table
//! reaches no crate it does not name, whatever crates the probe session
//! that grew it loaded, so an entry's metadata records only these.

use std::collections::BTreeSet;
use std::fmt;
use std::path::Path;

use serde::de::{self, Deserialize, Deserializer, MapAccess, SeqAccess, Visitor};

use super::Meta;

/// One JSON value read for the crates it names: each one an object in it
/// names, and the value itself when it is a string.
struct Named {
    crates: BTreeSet<(String, String)>,
    text: Option<String>,
}

impl Named {
    const fn empty() -> Self {
        Self {
            crates: BTreeSet::new(),
            text: None,
        }
    }
}

impl<'de> Deserialize<'de> for Named {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        deserializer.deserialize_any(Walk)
    }
}

/// Visits any JSON value, keeping the crates each object in it names.
struct Walk;

impl<'de> Visitor<'de> for Walk {
    type Value = Named;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_bool<E: de::Error>(self, _: bool) -> Result<Named, E> {
        Ok(Named::empty())
    }

    fn visit_i64<E: de::Error>(self, _: i64) -> Result<Named, E> {
        Ok(Named::empty())
    }

    fn visit_u64<E: de::Error>(self, _: u64) -> Result<Named, E> {
        Ok(Named::empty())
    }

    fn visit_f64<E: de::Error>(self, _: f64) -> Result<Named, E> {
        Ok(Named::empty())
    }

    fn visit_unit<E: de::Error>(self) -> Result<Named, E> {
        Ok(Named::empty())
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Named, E> {
        Ok(Named {
            crates: BTreeSet::new(),
            text: Some(value.to_owned()),
        })
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Named, A::Error> {
        let mut crates = BTreeSet::new();
        while let Some(mut element) = sequence.next_element::<Named>()? {
            crates.append(&mut element.crates);
        }
        Ok(Named { crates, text: None })
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Named, A::Error> {
        let mut crates = BTreeSet::new();
        let mut crate_name = None;
        let mut stable_crate_id = None;
        while let Some(key) = map.next_key::<std::borrow::Cow<'de, str>>()? {
            let mut value = map.next_value::<Named>()?;
            crates.append(&mut value.crates);
            match &*key {
                "crate_name" => crate_name = value.text,
                "stable_crate_id" => stable_crate_id = value.text,
                _ => {}
            }
        }
        if let (Some(crate_name), Some(stable_crate_id)) = (crate_name, stable_crate_id) {
            crates.insert((crate_name, stable_crate_id));
        }
        Ok(Named { crates, text: None })
    }
}

/// The crates the table at `path` names; none when it can't be read.
fn crates(path: &Path) -> Option<BTreeSet<(String, String)>> {
    let bytes = std::fs::read(path).ok()?;
    crate::extract::sidecar::read::<Named>(&bytes)
        .ok()
        .map(|named| named.crates)
}

impl Meta {
    /// Keep only the external crates the table at `table` names. A table
    /// that can't be read keeps them all: no replay of it reads any.
    pub(super) fn retain_named(&mut self, table: &Path) {
        let Some(named) = crates(table) else {
            return;
        };
        self.externals.retain(|external| {
            named
                .iter()
                .any(|(name, id)| *name == external.crate_name && *id == external.stable_crate_id)
        });
    }
}

#[cfg(test)]
mod tests {
    use super::Named;

    #[test]
    fn a_table_names_each_crate_an_object_in_it_carries_with_its_stable_id() {
        let table = br#"{"schema":9,"units":[[{"key":1},[{"Register":{"sysroot":false,"crate_name":"meters","stable_crate_id":"aa"}}]]],"modules":[{"children":[{"export":{"hops":[{"sysroot":true,"crate_name":"core","stable_crate_id":"bb"}],"target":{"id":{"authority":{"kind":"compiler","crate_name":"meters","stable_crate_id":"aa"}}}}}]}],"crate_name":"alone"}"#;
        let named = crate::extract::sidecar::read::<Named>(table).unwrap();
        let crates = named
            .crates
            .iter()
            .map(|(name, id)| format!("{name}-{id}"))
            .collect::<Vec<_>>();
        assert_eq!(crates, ["core-bb", "meters-aa"]);
    }
}

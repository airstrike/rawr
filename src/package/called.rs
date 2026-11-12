//! The marked items native code names, as the checks of the generated
//! crates recorded them (D942, `rawr_import::uses`). rawr's copy of the
//! crate gives a trampoline its body only when native code names its
//! function, and every other one its signature over a body that never runs
//! (`island::shadow`, #1807), so the copy type-checks as little as native
//! code needs. `rawr check` keeps the record beside the project's other
//! rawr files ([`store`]); a check that finds native code naming a function
//! the record lacks keeps it and checks again with its body, as a new
//! instance is (D869).
//!
//! The record only grows: a body native code no longer names is kept, so a
//! save that drops a use writes no new copy. Any record is a safe guess,
//! as the check corrects it, so one another build of rawr kept is read as
//! this build's.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The record's layout. A record of another is read as none.
const SCHEMA: u32 = 1;

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: u32,
    named: BTreeSet<String>,
}

/// Where the project at `root` keeps its record.
fn location(root: &Path) -> PathBuf {
    rawr_import::location::rawr_directory_here(root).join("called.json")
}

/// The marked items the project at `root` last recorded native code
/// naming, or `None` when there is no record, so every trampoline gets its
/// body.
#[must_use]
pub fn kept(root: &Path) -> Option<BTreeSet<String>> {
    let bytes = fs::read(location(root)).ok()?;
    let record = serde_json::from_slice::<Record>(&bytes).ok()?;
    (record.schema == SCHEMA).then_some(record.named)
}

/// Keep `named`, what a check found native code naming, with what the
/// project kept before; whether the record grew, so rawr's copy must be
/// written again with the bodies it lacks and checked again.
///
/// # Errors
///
/// The record can't be written.
pub fn store(root: &Path, named: &BTreeSet<String>) -> io::Result<bool> {
    let kept = kept(root);
    if kept.as_ref().is_some_and(|kept| kept.is_superset(named)) {
        return Ok(false);
    }
    let mut record = Record {
        schema: SCHEMA,
        named: kept.unwrap_or_default(),
    };
    record.named.extend(named.iter().cloned());
    let path = location(root);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(
        &path,
        serde_json::to_vec(&record).map_err(io::Error::other)?,
    )?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    #[test]
    fn a_record_grows_with_what_a_check_names_and_never_shrinks() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/tmp")
            .join(format!("called-grows-{}", std::process::id()));
        let named = |names: &[&str]| names.iter().map(|name| (*name).to_owned()).collect();
        assert_eq!(super::kept(&root), None);
        let first: BTreeSet<String> = named(&["home::heading"]);
        // The first record grows from none, an empty one too.
        assert!(super::store(&root, &BTreeSet::new()).expect("the record is stored"));
        assert_eq!(super::kept(&root), Some(BTreeSet::new()));
        assert!(super::store(&root, &first).expect("the record is stored"));
        assert_eq!(super::kept(&root).as_ref(), Some(&first));
        assert!(!super::store(&root, &first).expect("the record is stored"));
        // A check that names less keeps what was kept.
        assert!(!super::store(&root, &BTreeSet::new()).expect("the record is stored"));
        assert!(super::store(&root, &named(&["home::view"])).expect("the record is stored"));
        assert_eq!(
            super::kept(&root),
            Some(named(&["home::heading", "home::view"]))
        );
        std::fs::remove_file(super::location(&root)).expect("the record is removed");
    }
}

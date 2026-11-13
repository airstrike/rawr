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
//!
//! Beside them, the record keeps the marked types the last check found
//! native code holding but never opening (`Uses::opaque`), which rawr's
//! copy declares as the handle of the program value (`island::handle`,
//! D961). That set is the last check's alone: a type native code starts
//! opening keeps its real layout from the next copy on, and a check that
//! finds the set changed checks again with the copy it implies.

use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The record's layout. A record of another is read as none.
const SCHEMA: u32 = 2;

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Record {
    schema: u32,
    named: BTreeSet<String>,
    opaque: BTreeSet<String>,
}

/// What a check found native code using of the marked modules, as rawr's
/// copy of the crate is written from it.
pub struct Used<'u> {
    /// The marked items native code names.
    pub named: &'u BTreeSet<String>,
    /// The marked types native code holds but never opens.
    pub opaque: &'u BTreeSet<String>,
}

/// Where the project at `root` keeps its record.
fn location(root: &Path) -> PathBuf {
    location_in(&rawr_import::location::rawr_directory_here(root))
}

/// Where the project whose Rawr directory is `rawr` keeps its record.
fn location_in(rawr: &Path) -> PathBuf {
    rawr.join("called.json")
}

/// The marked items the project at `root` last recorded native code
/// naming, or `None` when there is no record, so every trampoline gets its
/// body.
#[must_use]
pub fn kept(root: &Path) -> Option<BTreeSet<String>> {
    read(&location(root)).map(|record| record.named)
}

/// [`kept`] for the project whose Rawr directory is `rawr`.
#[must_use]
pub fn kept_in(rawr: &Path) -> Option<BTreeSet<String>> {
    read(&location_in(rawr)).map(|record| record.named)
}

/// The marked types the last check of the project whose Rawr directory is
/// `rawr` found native code holding but never opening, or `None` when there
/// is no record, so every type keeps its real layout.
#[must_use]
pub fn opaque_in(rawr: &Path) -> Option<BTreeSet<String>> {
    read(&location_in(rawr)).map(|record| record.opaque)
}

/// The record at `path`.
fn read(path: &Path) -> Option<Record> {
    let bytes = fs::read(path).ok()?;
    let record = serde_json::from_slice::<Record>(&bytes).ok()?;
    (record.schema == SCHEMA).then_some(record)
}

/// Keep `used`, what a check found, with the names the project kept
/// before; whether rawr's copy must be written again from it and checked
/// again: the names grew, or the opaque types changed.
///
/// # Errors
///
/// The record can't be written.
pub fn store(root: &Path, used: &Used<'_>) -> io::Result<bool> {
    write(&location(root), used)
}

/// [`store`] for the project whose Rawr directory is `rawr`.
///
/// # Errors
///
/// The record can't be written.
pub fn store_in(rawr: &Path, used: &Used<'_>) -> io::Result<bool> {
    write(&location_in(rawr), used)
}

/// Keep `used` with the record at `path`.
fn write(path: &Path, used: &Used<'_>) -> io::Result<bool> {
    let kept = read(path);
    if kept
        .as_ref()
        .is_some_and(|kept| kept.named.is_superset(used.named) && kept.opaque == *used.opaque)
    {
        return Ok(false);
    }
    let mut record = Record {
        schema: SCHEMA,
        named: kept.map(|kept| kept.named).unwrap_or_default(),
        opaque: used.opaque.clone(),
    };
    record.named.extend(used.named.iter().cloned());
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec(&record).map_err(io::Error::other)?)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::Used;

    /// The names grow with each check and never shrink; the opaque types
    /// are the last check's.
    #[test]
    fn a_record_grows_with_what_a_check_names_and_never_shrinks() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("target/tmp")
            .join(format!("called-grows-{}", std::process::id()));
        let named = |names: &[&str]| names.iter().map(|name| (*name).to_owned()).collect();
        let none = BTreeSet::new();
        let used = |named| Used {
            named,
            opaque: &none,
        };
        assert_eq!(super::kept(&root), None);
        let first: BTreeSet<String> = named(&["home::heading"]);
        let view: BTreeSet<String> = named(&["home::view"]);
        // The first record grows from none, an empty one too.
        assert!(super::store(&root, &used(&none)).expect("the record is stored"));
        assert_eq!(super::kept(&root), Some(BTreeSet::new()));
        assert!(super::store(&root, &used(&first)).expect("the record is stored"));
        assert_eq!(super::kept(&root).as_ref(), Some(&first));
        assert!(!super::store(&root, &used(&first)).expect("the record is stored"));
        // A check that names less keeps what was kept.
        assert!(!super::store(&root, &used(&none)).expect("the record is stored"));
        assert!(super::store(&root, &used(&view)).expect("the record is stored"));
        assert_eq!(
            super::kept(&root),
            Some(named(&["home::heading", "home::view"]))
        );
        // The types native code holds but never opens are the last
        // check's: a check that finds them changed, more or fewer, writes
        // them and says the copy is written again.
        let rawr = rawr_import::location::rawr_directory_here(&root);
        let opaque: BTreeSet<String> = named(&["home::State"]);
        let found = Used {
            named: &view,
            opaque: &opaque,
        };
        assert_eq!(super::opaque_in(&rawr), Some(BTreeSet::new()));
        assert!(super::store(&root, &found).expect("the record is stored"));
        assert_eq!(super::opaque_in(&rawr).as_ref(), Some(&opaque));
        assert!(!super::store(&root, &found).expect("the record is stored"));
        assert!(super::store(&root, &used(&view)).expect("the record is stored"));
        assert_eq!(super::opaque_in(&rawr), Some(BTreeSet::new()));
        std::fs::remove_file(super::location(&root)).expect("the record is removed");
    }
}

//! Verify the ordinary saved-output digest before relocating its structured data.

use std::io;

use serde::ser::{SerializeSeq as _, SerializeStruct as _};
use serde::{Serialize, Serializer};
use sha2::{Digest as _, Sha256};

use super::Account;
use crate::cargo::{universe::MetadataProof, Limits};
use crate::input::build::Evidence;
use crate::input::proof::Entry;

pub(super) fn project<A: Account>(
    entries: &[&Entry],
    package: &str,
    execution: &str,
    expected: &str,
    evidence: &Evidence,
    protected: &MetadataProof,
    limits: &Limits,
    account: &mut A,
) -> Result<Box<str>, A::Error> {
    // Reserve bounded borrowed slots once, avoiding a second full scan of all
    // other executions merely to determine this execution's exact capacity.
    let mut ordered = account.roots(entries.len())?;
    for entry in entries {
        if let Entry::Directive {
            package_id,
            execution: candidate,
            index,
            value,
        } = entry
        {
            account.comparison(package_id.len().saturating_add(candidate.len()))?;
            if package_id.as_ref() == package && candidate.as_ref() == execution {
                ordered.push((*index, value.as_ref()));
            }
        }
    }
    let count = ordered.len();
    account.comparison(
        count
            .saturating_mul(count.max(1).ilog2().saturating_add(1) as usize)
            .saturating_mul(std::mem::size_of::<usize>()),
    )?;
    ordered.sort_unstable_by_key(|(index, _)| *index);
    let mut original = account.roots(count)?;
    for (expected_index, (index, value)) in ordered.into_iter().enumerate() {
        if index != expected_index {
            return Err(account.policy(
                "protected-saved-output-order",
                "missing or duplicate directive index",
            ));
        }
        original.push(value);
    }
    let digest = hash(&Saved(&original), limits, account)?;
    let actual = account.format(format_args!("{digest:x}"))?;
    if actual != expected {
        return Err(account.policy(
            "protected-saved-output-digest",
            "directives do not reproduce the ordinary saved-output digest",
        ));
    }
    let mut normalized = account.roots(count)?;
    for value in original {
        let value = account.normalized(value, evidence, protected)?;
        normalized.push(account.boxed(value)?);
    }
    let digest = hash(&Saved(&normalized), limits, account)?;
    let value = account.format(format_args!("{digest:x}"))?;
    account.boxed(value)
}

// These fields and their order are the ordinary replay::SavedOutput schema.
// Rerun arrays are losslessly derived from the ordered directives by the pinned
// scanner, including duplicate declarations and their original order.
struct Saved<'a, T>(&'a [T]);
struct Rerun<'a, T>(&'a [T], &'static str);
impl<T: AsRef<str> + Serialize> Serialize for Saved<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut value = serializer.serialize_struct("SavedOutput", 3)?;
        value.serialize_field("directives", self.0)?;
        value.serialize_field("paths", &Rerun(self.0, "rerun-if-changed="))?;
        value.serialize_field("environment", &Rerun(self.0, "rerun-if-env-changed="))?;
        value.end()
    }
}
impl<T: AsRef<str>> Serialize for Rerun<'_, T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut sequence = serializer.serialize_seq(None)?;
        for value in self.0 {
            if let Some(value) = value.as_ref().strip_prefix(self.1) {
                sequence.serialize_element(value)?;
            }
        }
        sequence.end()
    }
}

fn hash<A: Account>(
    value: &impl Serialize,
    limits: &Limits,
    account: &mut A,
) -> Result<sha2::digest::Output<Sha256>, A::Error> {
    let mut writer = Writer {
        hash: None,
        bytes: 0,
        overflow: false,
    };
    serde_json::to_writer(&mut writer, value)
        .map_err(|_| account.policy("protected-saved-output-encoding", "serialization failed"))?;
    if writer.overflow {
        return Err(account.policy("protected-saved-output-size", "serialized size overflow"));
    }
    account.limited(
        "protected saved-output bytes",
        limits.json_bytes,
        writer.bytes,
    )?;
    account.hash_work(writer.bytes)?;
    writer.hash = Some(Sha256::new());
    serde_json::to_writer(&mut writer, value)
        .map_err(|_| account.policy("protected-saved-output-encoding", "serialization failed"))?;
    writer
        .hash
        .map(|hash| hash.finalize())
        .ok_or_else(|| account.policy("protected-saved-output-hash", "hash state missing"))
}
struct Writer {
    hash: Option<Sha256>,
    bytes: usize,
    overflow: bool,
}
impl io::Write for Writer {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if let Some(hash) = &mut self.hash {
            hash.update(bytes);
        } else if let Some(size) = self.bytes.checked_add(bytes.len()) {
            self.bytes = size;
        } else {
            self.overflow = true;
        }
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::proof::protected::account::{Current, Legacy};
    use crate::input::stored::Budget;
    use crate::input::{Allocation, Usage};
    use crate::node::GraphBudget;

    fn entries(values: &[&str]) -> Vec<Entry> {
        values
            .iter()
            .enumerate()
            .map(|(index, value)| Entry::Directive {
                package_id: "package".into(),
                execution: "/out".into(),
                index,
                value: (*value).into(),
            })
            .collect()
    }
    fn expected(values: &[&str]) -> String {
        format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&Saved(values)).expect("serialize"))
        )
    }
    fn fixture() -> (Evidence, MetadataProof) {
        (
            Evidence {
                usage: Usage { items: 0, bytes: 0 },
                packages: Default::default(),
                scripts: Box::default(),
                artifacts: Box::default(),
                environment: Box::default(),
            },
            MetadataProof::fixture_package("package", "package", "1.0.0", "path", "/root".into()),
        )
    }
    #[test]
    fn ordered_duplicates_and_empty_output_match_both_accounts() {
        for values in [
            vec![],
            vec![
                "rerun-if-changed=a",
                "rerun-if-env-changed=X",
                "rerun-if-changed=a",
                "rustc-link-lib=static=x",
            ],
        ] {
            let mut entries = entries(&values);
            entries.reverse();
            let refs: Vec<_> = entries.iter().collect();
            let (evidence, protected) = fixture();
            let limits = Limits::standard();
            let graph = GraphBudget::new();
            let mut allocation = Allocation::new(&limits);
            let cold = project(
                &refs,
                "package",
                "/out",
                &expected(&values),
                &evidence,
                &protected,
                &limits,
                &mut Legacy(&mut allocation, &graph),
            )
            .expect("cold");
            let mut current_allocation = Allocation::new(&limits);
            let mut budget = Budget::borrow(&limits, &mut current_allocation, &graph);
            let current = project(
                &refs,
                "package",
                "/out",
                &expected(&values),
                &evidence,
                &protected,
                &limits,
                &mut Current(&mut budget),
            )
            .expect("current");
            assert_eq!(cold, current);
            assert_eq!(cold.as_ref(), expected(&values));
        }
    }
    #[test]
    fn missing_duplicate_or_changed_directives_reject() {
        let values = ["rustc-link-lib=static=x", "rerun-if-changed=a"];
        let original = expected(&values);
        for mode in 0..4 {
            let mut entries = entries(&values);
            match mode {
                0 => {
                    entries.pop();
                }
                1 => {
                    entries.remove(0);
                }
                2 => {
                    if let Entry::Directive { index, .. } = &mut entries[1] {
                        *index = 0;
                    }
                }
                _ => {
                    if let Entry::Directive { value, .. } = &mut entries[0] {
                        *value = "rustc-link-lib=static=y".into();
                    }
                }
            }
            let refs: Vec<_> = entries.iter().collect();
            let (evidence, protected) = fixture();
            let limits = Limits::standard();
            let graph = GraphBudget::new();
            let mut allocation = Allocation::new(&limits);
            assert!(project(
                &refs,
                "package",
                "/out",
                &original,
                &evidence,
                &protected,
                &limits,
                &mut Legacy(&mut allocation, &graph)
            )
            .is_err());
        }
    }
    #[test]
    fn relocation_preserves_identity_but_directive_changes_do_not() {
        fn run(root: &str, library: &str) -> Box<str> {
            let directive = format!("rustc-link-search=native={root}/native");
            let values = [directive.as_str(), library];
            let entries = entries(&values);
            let refs: Vec<_> = entries.iter().collect();
            let (mut evidence, _) = fixture();
            evidence.scripts = Box::from([crate::input::build::Script {
                compiler: None,
                package_id: "package".into(),
                out_dir: root.into(),
                cfg: Box::default(),
                environment: Box::default(),
            }]);
            let protected = MetadataProof::fixture_package(
                "package",
                "package",
                "1.0.0",
                "path",
                "/source".into(),
            );
            let limits = Limits::standard();
            let graph = GraphBudget::new();
            let mut allocation = Allocation::new(&limits);
            project(
                &refs,
                "package",
                "/out",
                &expected(&values),
                &evidence,
                &protected,
                &limits,
                &mut Legacy(&mut allocation, &graph),
            )
            .expect("relocated projection")
        }
        assert_eq!(
            run("/root/a", "rustc-link-lib=static=x"),
            run("/root/b", "rustc-link-lib=static=x")
        );
        assert_ne!(
            run("/root/a", "rustc-link-lib=static=x"),
            run("/root/b", "rustc-link-lib=static=y")
        );
    }

    #[test]
    fn overflowed_original_hash_accounts_refuse_and_latch() {
        let serialized = serde_json::to_vec(&Saved::<&str>(&[]))
            .expect("serialize")
            .len() as u64;
        for current in [false, true] {
            let (evidence, protected) = fixture();
            let limits = Limits::standard();
            let graph = GraphBudget::new();
            graph
                .hash(
                    usize::try_from(crate::node::MAX_HASHED_BYTES_PER_GRAPH)
                        .expect("test platform"),
                )
                .expect("exhaust hash account");
            let mut allocation = Allocation::new(&limits);
            // Hash work past the target is recorded on the graph with
            // the exact serialized size (the original digest first, then the
            // normalized one); both accounts still project and neither latches.
            let projected = if current {
                let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
                project(
                    &[],
                    "package",
                    "/out",
                    &expected(&[]),
                    &evidence,
                    &protected,
                    &limits,
                    &mut Current(&mut budget),
                )
            } else {
                project(
                    &[],
                    "package",
                    "/out",
                    &expected(&[]),
                    &evidence,
                    &protected,
                    &limits,
                    &mut Legacy(&mut allocation, &graph),
                )
                .map_err(|_| crate::input::stored::Error::Refused)
            };
            assert_eq!(
                projected
                    .expect("the hash target is recorded, not refused")
                    .as_ref(),
                expected(&[])
            );
            let axis = *graph
                .exceedances()
                .axis(crate::node::HASHED_RESOURCE)
                .expect("saved-output hashing records the hash axis");
            assert_eq!(
                (axis.target, axis.first_requested, axis.high_water),
                (
                    crate::node::MAX_HASHED_BYTES_PER_GRAPH,
                    crate::node::MAX_HASHED_BYTES_PER_GRAPH + serialized,
                    crate::node::MAX_HASHED_BYTES_PER_GRAPH + 2 * serialized
                )
            );
            allocation
                .check()
                .expect("an accounting exceedance never latches the account");
            // Checked-arithmetic overflow of the hash account is a genuine
            // failure: both accounts refuse and latch the original allocation.
            let hashed = graph.census_usage().expect("graph census").2;
            graph
                .hash(usize::try_from(u64::MAX - hashed).expect("addressable remainder"))
                .expect("saturate the hash account");
            if current {
                let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
                assert!(matches!(
                    project(
                        &[],
                        "package",
                        "/out",
                        &expected(&[]),
                        &evidence,
                        &protected,
                        &limits,
                        &mut Current(&mut budget)
                    ),
                    Err(crate::input::stored::Error::Graph(
                        crate::node::Error::Allocation
                    ))
                ));
            } else {
                assert!(matches!(
                    project(
                        &[],
                        "package",
                        "/out",
                        &expected(&[]),
                        &evidence,
                        &protected,
                        &limits,
                        &mut Legacy(&mut allocation, &graph)
                    ),
                    Err(crate::Error::Input {
                        code: "protected-build-hash-work",
                        ..
                    })
                ));
            }
            assert!(allocation.check().is_err());
        }
    }
    #[test]
    fn unrelated_directives_are_selected_in_one_budgeted_pass() {
        let entries: Vec<_> = (0..200)
            .map(|index| Entry::Directive {
                package_id: format!(
                    "registry+https://example.invalid/{}#other@1.0.0",
                    "p".repeat(100)
                )
                .into(),
                execution: format!("/build/{}/out", "x".repeat(100)).into(),
                index,
                value: "rustc-link-lib=static=other".into(),
            })
            .collect();
        let comparison: usize = entries
            .iter()
            .map(|entry| match entry {
                Entry::Directive {
                    package_id,
                    execution,
                    ..
                } => package_id.len() + execution.len(),
                _ => 0,
            })
            .sum();
        let available = comparison + entries.len() * std::mem::size_of::<(usize, &str)>() + 4096;
        assert!(available < comparison * 2, "the old two scans cannot fit");
        let refs: Vec<_> = entries.iter().collect();
        let (evidence, protected) = fixture();
        let limits = Limits::standard();
        let graph = GraphBudget::new();
        graph
            .decode(
                usize::try_from(crate::node::MAX_DECODED_BYTES_PER_GRAPH).expect("test range")
                    - available,
            )
            .expect("prior graph work");
        let mut allocation = Allocation::new(&limits);
        let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
        let result = project(
            &refs,
            "package",
            "/out",
            &expected(&[]),
            &evidence,
            &protected,
            &limits,
            &mut Current(&mut budget),
        )
        .expect("one-pass selection fits remaining original budget");
        assert_eq!(result.as_ref(), expected(&[]));
    }
}

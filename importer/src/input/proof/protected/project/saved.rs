//! Verify the ordinary saved-output digest before relocating its structured data.

use std::io;

use serde::ser::{SerializeSeq as _, SerializeStruct as _};
use serde::{Serialize, Serializer};
use sha2::{Digest as _, Sha256};

use super::Account;
use crate::cargo::universe::MetadataProof;
use crate::input::build::Evidence;
use crate::input::proof::Entry;

#[allow(clippy::too_many_arguments)]
pub(super) fn project<A: Account>(
    entries: &[&Entry],
    package: &str,
    execution: &str,
    expected: &str,
    evidence: &Evidence,
    protected: &MetadataProof,
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
            account.check()?;
            if package_id.as_ref() == package && candidate.as_ref() == execution {
                ordered.push((*index, value.as_ref()));
            }
        }
    }
    let count = ordered.len();
    account.check()?;
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
    let digest = hash(&Saved(&original), account)?;
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
    let digest = hash(&Saved(&normalized), account)?;
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
    account.check()?;
    account.check()?;
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
    use crate::cargo::Limits;
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
                &mut Legacy(&mut allocation),
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
            let mut allocation = Allocation::new(&limits);
            assert!(project(
                &refs,
                "package",
                "/out",
                &original,
                &evidence,
                &protected,
                &mut Legacy(&mut allocation)
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
            let mut allocation = Allocation::new(&limits);
            project(
                &refs,
                "package",
                "/out",
                &expected(&values),
                &evidence,
                &protected,
                &mut Legacy(&mut allocation),
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
        let mut allocation = Allocation::new(&limits);
        let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
        let result = project(
            &refs,
            "package",
            "/out",
            &expected(&[]),
            &evidence,
            &protected,
            &mut Current(&mut budget),
        )
        .expect("one-pass selection fits remaining original budget");
        assert_eq!(result.as_ref(), expected(&[]));
    }
}

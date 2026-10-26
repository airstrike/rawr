//! Structured replay of pinned rustc dep-info and Cargo saved output.

use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest as _, Sha256};

use super::proof::{Entry, Role};
use super::Allocation;
use crate::cargo::{self, toolchain};
use crate::{Error, Result};

pub(in crate::input) mod canonical;
pub(in crate::input) mod observed;

#[derive(Serialize)]
struct Dependency<'dependency, Source> {
    sources: &'dependency [Source],
    #[allow(
        clippy::type_complexity,
        reason = "the replay schema retains exact key and optional-value dep-info tuples"
    )]
    environment: &'dependency [(Box<str>, Option<Box<str>>)],
}

#[derive(Serialize)]
struct SavedOutput<'declarations> {
    directives: &'declarations [Box<str>],
    paths: &'declarations [Box<str>],
    environment: &'declarations [Box<str>],
}

impl<'declarations> SavedOutput<'declarations> {
    fn borrow(declarations: &'declarations super::declaration::Declarations) -> Self {
        Self {
            directives: &declarations.directives,
            paths: &declarations.paths,
            environment: &declarations.environment,
        }
    }
}

pub(super) fn dep_info_entry(
    package_id: &str,
    path: &Path,
    package_root: &Path,
    out_directories: &[PathBuf],
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> Result<Entry> {
    let dependency = super::dependency::Dependency::read(
        toolchain::supported_cargo_commit(),
        path,
        package_root,
        limits,
        allocation,
    )?;
    for (key, value) in &dependency.environment {
        if key.as_ref() == "OUT_DIR" {
            let value = value.as_deref().ok_or_else(|| {
                super::input("unproved-out-dir-env-dep", "OUT_DIR is unexpectedly unset")
            })?;
            let out_dir = super::canonical_directory("rustc OUT_DIR env-dep", Path::new(value))?;
            if !out_directories
                .iter()
                .any(|candidate| candidate == &out_dir)
            {
                return Err(super::input(
                    "unproved-out-dir-env-dep",
                    format_args!(
                        "OUT_DIR `{}` does not identify the Build execution of `{package_id}`",
                        out_dir.display()
                    ),
                ));
            }
        }
    }
    allocation.items(dependency.sources.len())?;
    let mut sources = Vec::<Box<str>>::new();
    sources
        .try_reserve_exact(dependency.sources.len())
        .map_err(|error| Error::Input {
            code: "structured-replay-allocation",
            reason: error.to_string().into_boxed_str(),
        })?;
    for source in &dependency.sources {
        allocation.path(source)?;
        sources.push(super::path_text("dep-info source path", source)?.into());
    }
    let sources = sources.into_boxed_slice();
    let evidence = Dependency {
        sources: &sources,
        environment: &dependency.environment,
    };
    fact(
        Role::DepInfo,
        package_id,
        path,
        &evidence,
        limits,
        allocation,
    )
}

pub(super) fn saved_output_entry(
    package_id: &str,
    out_dir: &Path,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> Result<Entry> {
    let declarations = super::declaration::Declarations::read_accounted(
        toolchain::supported_cargo_commit(),
        package_id,
        out_dir,
        limits,
        allocation,
    )?;
    let evidence = SavedOutput::borrow(&declarations);
    let digest = structured_digest(&evidence, limits, allocation)?;
    allocation.string(package_id)?;
    allocation.path(out_dir)?;
    allocation.string("saved-output")?;
    allocation.path(&declarations.output)?;
    allocation.bytes(digest.len().saturating_mul(2))?;
    Ok(Entry::BuildMessage {
        package_id: package_id.into(),
        execution: super::path_text("saved output execution", out_dir)?.into(),
        name: "saved-output".into(),
        source: Some(super::path_text("Cargo saved build output", &declarations.output)?.into()),
        value: hex(digest).into_boxed_str(),
    })
}

fn fact(
    role: Role,
    package_id: &str,
    path: &Path,
    evidence: &impl Serialize,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> Result<Entry> {
    let digest = structured_digest(evidence, limits, allocation)?;
    allocation.path(path)?;
    let path = super::path_text("structured replay path", path)?;
    let name = allocation.formatted(format_args!("{package_id}:{path}"))?;
    allocation.items(1)?;
    allocation.bytes(digest.len().saturating_mul(2))?;
    Ok(Entry::Fact {
        role,
        package_id: Some(package_id.into()),
        name: name.into_boxed_str(),
        value: hex(digest).into_boxed_str(),
    })
}

fn structured_digest(
    evidence: &impl Serialize,
    limits: &cargo::Limits,
    allocation: &mut Allocation,
) -> Result<[u8; 32]> {
    struct Count(usize);
    impl std::io::Write for Count {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .ok_or_else(|| std::io::Error::other("structured replay length overflowed"))?;
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut count = Count(0);
    serde_json::to_writer(&mut count, evidence).map_err(|error| Error::Input {
        code: "structured-replay-encoding",
        reason: error.to_string().into_boxed_str(),
    })?;
    allocation.exceeded("structured replay bytes", limits.json_bytes, count.0);
    allocation.items(1)?;
    allocation.bytes(count.0)?;
    let mut canonical = Vec::new();
    canonical
        .try_reserve_exact(count.0)
        .map_err(|error| Error::Input {
            code: "structured-replay-allocation",
            reason: error.to_string().into_boxed_str(),
        })?;
    serde_json::to_writer(&mut canonical, evidence).map_err(|error| Error::Input {
        code: "structured-replay-encoding",
        reason: error.to_string().into_boxed_str(),
    })?;
    allocation.items(1)?;
    Ok(Sha256::digest(canonical).into())
}

fn hex(bytes: impl AsRef<[u8]>) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.as_ref().len().saturating_mul(2));
    for byte in bytes.as_ref() {
        encoded.push(char::from(DIGITS[usize::from(byte >> 4)]));
        encoded.push(char::from(DIGITS[usize::from(byte & 0x0f)]));
    }
    encoded
}

#[cfg(test)]
mod tests {
    use super::structured_digest;
    use crate::cargo::Limits;
    use crate::input::Allocation;

    /// The structured replay size is an accounting target on the input
    /// account; exact records nothing, one under digests the same bytes and
    /// records the axis with the truthful size.
    #[test]
    fn structured_replay_bytes_record_the_json_target_without_refusal() {
        let evidence = ("structured replay fixture", 7_u32);
        let length = serde_json::to_vec(&evidence).unwrap().len();
        let mut limits = Limits::standard();
        limits.json_bytes = length;
        let mut allocation = Allocation::for_replay(&limits);
        let exact = structured_digest(&evidence, &limits, &mut allocation)
            .expect("a document at the JSON target digests");
        assert!(allocation
            .exceedances()
            .axis("structured replay bytes")
            .is_none());
        limits.json_bytes = length - 1;
        let mut allocation = Allocation::for_replay(&limits);
        let floored = structured_digest(&evidence, &limits, &mut allocation)
            .expect("a document one byte over the JSON target digests");
        assert_eq!(exact, floored);
        let axis = *allocation
            .exceedances()
            .axis("structured replay bytes")
            .expect("the document size records its axis");
        assert_eq!(
            (axis.target, axis.first_requested, axis.high_water),
            ((length - 1) as u64, length as u64, length as u64)
        );
    }
}

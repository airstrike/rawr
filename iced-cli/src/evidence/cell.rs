//! One-shot child admission and normal imported-owner dispatch.

use std::ffi::OsStr;
use std::fs;
use std::path::{Path, PathBuf};

use rawr_import::metrics::cell::{self, Outcome, Profile, Session};
use sha2::{Digest as _, Sha256};

use super::CellPending;

const FIXTURE_DIGEST: &str = "a543318f512e336f058b55895a81f62ba77d77821cd982983c315f03c308faa5";

pub(super) fn execute(pending: CellPending) -> Result<(), Error> {
    let executable = std::env::current_exe().map_err(Error::Io)?;
    let expected_build = build(&executable)?;
    let fixture_digest = fixture_digest()?;
    let session = Session::transform(
        &pending.socket,
        pending.launch_id,
        pending.request_digest,
        &expected_build,
        fixture_digest,
    )?;
    let request = session.request();
    let project = request.core.project.path()?;
    let recorder = rawr_import::metrics::recorder::Recorder::transform_at(
        request.core.bounds.counter_limit,
        request.core.launched_ns,
    )?;
    let edge = Edge {
        project,
        owner: request.core.owner,
        profile: match request.core.profile {
            Profile::Inactive => None,
            Profile::Baseline => Some(rawr_iced::profile::Fixture::Baseline),
            Profile::Variant => Some(rawr_iced::profile::Fixture::Variant),
        },
    };
    let owner_entry_ns = monotonic()?;
    let result = {
        let _scope = recorder.scope()?;
        edge.invoke()
    };
    finish_owner(result, |outcome| {
        let owner_return_ns = monotonic()?;
        let counters = recorder.snapshot()?;
        session.finish(owner_entry_ns, owner_return_ns, counters, outcome)?;
        Ok(())
    })
}

fn finish_owner(
    result: Result<[u8; 32], Box<str>>,
    report: impl FnOnce(Outcome) -> Result<(), Error>,
) -> Result<(), Error> {
    let outcome = match &result {
        Ok(generation) => Outcome::Success {
            generation: *generation,
        },
        Err(reason) => Outcome::Failure {
            reason: reason.clone(),
        },
    };
    // Reporting still runs on a failed owner, but can never replace its earlier
    // diagnostic with a clock, recorder, serialization, or transport failure.
    let reporting = report(outcome);
    result.map_err(Error::Owner)?;
    reporting
}

pub(super) fn build(executable: &Path) -> Result<cell::Build, Error> {
    // These are current-process corroborating facts. The parent separately
    // verifies the retained receipt from an actual successful Cargo invocation.
    let root = fs::canonicalize(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(".."))
        .map_err(Error::Io)?;
    let head = output_at(&root, "git", &["rev-parse", "HEAD"])?;
    let diff = std::process::Command::new("git")
        .current_dir(&root)
        .args([
            "diff",
            "--no-ext-diff",
            "--binary",
            "--",
            ".",
            ":(exclude)SOL_PROMPT.md",
            ":(exclude)editors/vscode-rawr/rawr-language-0.0.1.vsix",
        ])
        .output()
        .map_err(Error::Io)?;
    if !diff.status.success() {
        return Err(Error::Environment {
            program: "git",
            reason: bounded(&diff.stderr),
        });
    }
    let lockfile = fs::read(root.join("Cargo.lock")).map_err(Error::Io)?;
    let source_tree = format!(
        "{}@{} tracked-diff-sha256={}",
        root.display(),
        head,
        rawr_import::metrics::evidence::hex(&Sha256::digest(diff.stdout)),
    );
    cell::Build::transform(executable, source_tree, Sha256::digest(lockfile).into())
        .map_err(Error::Protocol)
}

struct Edge {
    project: PathBuf,
    owner: rawr_import::metrics::report::Command,
    profile: Option<rawr_iced::profile::Fixture>,
}

impl Edge {
    fn invoke(self) -> Result<[u8; 32], Box<str>> {
        let target = rawr::project::Target::discover(&self.project)
            .map_err(|error| bounded(error.to_string().as_bytes()))?;
        crate::import::owner::cell(&target, self.owner, self.profile)
            .map_err(|report| bounded(crate::diagnostic::render(&report).as_bytes()))
    }
}

fn output_at(root: &Path, program: &'static str, arguments: &[&str]) -> Result<Box<str>, Error> {
    let output = std::process::Command::new(program)
        .current_dir(root)
        .args(arguments)
        .output()
        .map_err(Error::Io)?;
    if !output.status.success() {
        return Err(Error::Environment {
            program,
            reason: bounded(&output.stderr),
        });
    }
    let value = String::from_utf8(output.stdout).map_err(|error| Error::Environment {
        program,
        reason: bounded(error.to_string().as_bytes()),
    })?;
    let value = value.trim();
    if value.is_empty() || value.len() > 16 * 1024 {
        return Err(Error::Environment {
            program,
            reason: "output is empty or exceeds its fixed bound".into(),
        });
    }
    Ok(value.into())
}

fn fixture_digest() -> Result<[u8; 32], Error> {
    rawr_import::metrics::evidence::parse_digest(OsStr::new(FIXTURE_DIGEST))
        .map_err(Error::Evidence)
}

fn monotonic() -> Result<u64, Error> {
    rawr_import::metrics::monotonic_ns().ok_or(Error::Clock)
}

fn bounded(bytes: &[u8]) -> Box<str> {
    let text = String::from_utf8_lossy(bytes);
    let mut end = text.len().min(16 * 1024);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].into()
}

#[cfg(test)]
mod tests {
    use rawr_import::metrics::cell::{self, Outcome};

    use super::{bounded, finish_owner, Error};

    #[test]
    fn owner_failure_survives_every_reporting_failure() {
        for failure in [
            Error::Clock,
            Error::Protocol(cell::Error::Outcome),
            Error::Io(std::io::Error::from(std::io::ErrorKind::BrokenPipe)),
        ] {
            let result = finish_owner(Err("original owner refusal".into()), |outcome| {
                assert_eq!(
                    outcome,
                    Outcome::Failure {
                        reason: "original owner refusal".into(),
                    }
                );
                Err(failure)
            });
            assert!(
                matches!(result, Err(Error::Owner(reason)) if &*reason == "original owner refusal")
            );
        }
        assert!(matches!(
            finish_owner(Ok([1; 32]), |_| Err(Error::Clock)),
            Err(Error::Clock)
        ));
    }

    #[test]
    fn diagnostic_bound_uses_utf8_bytes_including_replacement_characters() {
        for bytes in [
            "\u{1f980}".repeat(10_000).into_bytes(),
            vec![0xff; 10_000],
            vec![b'a'; 20_000],
        ] {
            let reason = bounded(&bytes);
            assert!(!reason.is_empty());
            assert!(reason.len() <= 16 * 1024);
            assert!(String::from_utf8_lossy(&bytes).starts_with(&*reason));
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cell I/O failed")]
    Io(#[source] std::io::Error),
    #[error("cell environment command {program} failed: {reason}")]
    Environment {
        program: &'static str,
        reason: Box<str>,
    },
    #[error("cell process-shared clock is unavailable")]
    Clock,
    #[error("imported owner failed: {0}")]
    Owner(Box<str>),
    #[error(transparent)]
    Evidence(#[from] rawr_import::metrics::evidence::Error),
    #[error(transparent)]
    Recorder(#[from] rawr_import::metrics::recorder::Error),
    #[error(transparent)]
    Protocol(#[from] cell::Error),
}

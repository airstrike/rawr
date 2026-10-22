//! Private typed activation prefix for authenticated baseline evidence.

use std::ffi::{OsStr, OsString};
use std::path::PathBuf;

use rawr_import::metrics::evidence;

const PREFIX: &str = "--baseline-evidence-v1";
const PROFILE_PREFIX: &str = "--baseline-profile-v1";
const CELL_PREFIX: &str = "--baseline-cell-v1";

mod cell;

/// Closed private bootstrap domain selected before the ordinary parser.
pub enum Bootstrap {
    Command(Arguments),
    Cell(CellPending),
}

/// Arguments either untouched or stripped of one exact private prefix.
pub struct Arguments {
    forwarded: Box<[OsString]>,
    evidence: Option<Pending>,
    profile: Option<rawr_iced::profile::Fixture>,
}

/// Validated prefix facts awaiting the ordinary parser's typed `run` proof.
pub struct Pending {
    socket: PathBuf,
    launch_id: [u8; 32],
    command_digest: [u8; 32],
}

/// Exact child bootstrap awaiting authenticated request admission.
pub struct CellPending {
    socket: PathBuf,
    launch_id: [u8; 32],
    request_digest: [u8; 32],
}

/// Select one mutually exclusive private bootstrap without classifying argv.
pub fn bootstrap(arguments: impl IntoIterator<Item = OsString>) -> Result<Bootstrap, Error> {
    let arguments = arguments.into_iter().collect::<Vec<_>>();
    if arguments.first().is_some_and(|first| first == CELL_PREFIX) {
        if arguments.len() != 4 {
            return Err(Error::CellPrefix);
        }
        return Ok(Bootstrap::Cell(CellPending {
            socket: PathBuf::from(&arguments[1]),
            launch_id: evidence::parse_digest(&arguments[2]).map_err(Error::Evidence)?,
            request_digest: evidence::parse_digest(&arguments[3]).map_err(Error::Evidence)?,
        }));
    }
    Arguments::transform(arguments).map(Bootstrap::Command)
}

/// Execute exactly one authenticated deterministic owner child.
pub fn execute_cell(pending: CellPending) -> Result<(), Error> {
    cell::execute(pending).map_err(Error::Cell)
}

/// Corroborate current build facts after the parent verifies build provenance.
#[cfg(test)]
pub(crate) fn cell_build(
    executable: &std::path::Path,
) -> Result<rawr_import::metrics::cell::Build, Error> {
    cell::build(executable).map_err(Error::Cell)
}

impl Arguments {
    /// Strip only the exact versioned prefix while retaining every ordinary byte.
    pub fn transform(arguments: impl IntoIterator<Item = OsString>) -> Result<Self, Error> {
        let arguments = arguments.into_iter().collect::<Vec<_>>();
        let (profile, offset) = if arguments
            .first()
            .is_some_and(|first| first == PROFILE_PREFIX)
        {
            let profile = match arguments.get(1).and_then(|value| value.to_str()) {
                Some("baseline") => rawr_iced::profile::Fixture::Baseline,
                Some("variant") => rawr_iced::profile::Fixture::Variant,
                _ => return Err(Error::Profile),
            };
            (Some(profile), 2)
        } else {
            (None, 0)
        };
        let remaining = &arguments[offset..];
        if remaining.first().is_some_and(|first| first == CELL_PREFIX) {
            return Err(Error::CellPrefix);
        }
        let (forwarded, evidence) = if remaining.first().is_some_and(|first| first == PREFIX) {
            if remaining.len() < 4 || remaining[3] != OsStr::new("run") {
                return Err(Error::Prefix);
            }
            let socket = PathBuf::from(&remaining[1]);
            let launch_id = evidence::parse_digest(&remaining[2])?;
            let forwarded = remaining[3..].to_vec().into_boxed_slice();
            let command_digest = evidence::command_digest(&forwarded);
            (
                forwarded,
                Some(Pending {
                    socket,
                    launch_id,
                    command_digest,
                }),
            )
        } else {
            (remaining.to_vec().into_boxed_slice(), None)
        };
        Ok(Self {
            forwarded,
            evidence,
            profile,
        })
    }

    /// Separate preserved argv from the evidence proof and typed profile input.
    pub fn into_parts(
        self,
    ) -> (
        Box<[OsString]>,
        Option<Pending>,
        Option<rawr_iced::profile::Fixture>,
    ) {
        (self.forwarded, self.evidence, self.profile)
    }
}

/// Admit fixture authority only to commands backed by the imported owner.
pub fn authorize_profile(
    profile: Option<rawr_iced::profile::Fixture>,
    command: &crate::command::Command,
) -> Result<Option<rawr_iced::profile::Fixture>, Error> {
    if profile.is_some()
        && matches!(
            command,
            crate::command::Command::New { .. } | crate::command::Command::Doctor { .. }
        )
    {
        Err(Error::ProfileCommand)
    } else {
        Ok(profile)
    }
}

/// Reject a private profile selection unless the sole parser produced an owner.
pub fn authorize_request(
    profile: Option<rawr_iced::profile::Fixture>,
    request: &crate::command::Request,
) -> Result<Option<rawr_iced::profile::Fixture>, Error> {
    match request {
        crate::command::Request::Execute(command) => authorize_profile(profile, command),
        crate::command::Request::Help | crate::command::Request::Version => {
            if profile.is_some() {
                Err(Error::ProfileCommand)
            } else {
                Ok(None)
            }
        }
    }
}

impl Pending {
    /// Require the sole ordinary parser to prove `run`, then authenticate evidence.
    pub fn authenticate(
        self,
        command: &crate::command::Command,
    ) -> Result<evidence::Active, Error> {
        if !matches!(command, crate::command::Command::Run { .. }) {
            return Err(Error::Command);
        }
        evidence::Active::transform(&self.socket, self.launch_id, self.command_digest)
            .map_err(Error::Evidence)
    }
}

/// Invalid private activation. It is never part of the public command grammar.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("invalid private evidence prefix")]
    Prefix,
    #[error("invalid private profile prefix")]
    Profile,
    #[error("invalid private cell prefix")]
    CellPrefix,
    #[error("private profile selection has no imported owner")]
    ProfileCommand,
    #[error("private evidence activation did not parse as `run`")]
    Command,
    #[error(transparent)]
    Evidence(#[from] evidence::Error),
    #[error(transparent)]
    Cell(#[from] cell::Error),
}

#[cfg(test)]
mod tests {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt as _;

    use super::Arguments;

    #[test]
    fn cell_bootstrap_is_exact_and_mutually_exclusive() {
        let launch = "01".repeat(32);
        let request = "02".repeat(32);
        assert!(matches!(
            super::bootstrap([
                OsString::from("--baseline-cell-v1"),
                OsString::from("/tmp/cell.sock"),
                OsString::from(&launch),
                OsString::from(&request),
            ]),
            Ok(super::Bootstrap::Cell(_))
        ));
        for arguments in [
            vec![OsString::from("--baseline-cell-v1")],
            vec![
                OsString::from("--baseline-cell-v1"),
                OsString::from("/tmp/cell.sock"),
                OsString::from(&launch),
                OsString::from(&request),
                OsString::from("check"),
            ],
            vec![
                OsString::from("--baseline-profile-v1"),
                OsString::from("baseline"),
                OsString::from("--baseline-cell-v1"),
                OsString::from("/tmp/cell.sock"),
                OsString::from(&launch),
                OsString::from(&request),
            ],
        ] {
            assert!(super::bootstrap(arguments).is_err());
        }
    }

    #[test]
    fn inactive_arguments_are_unchanged() {
        let arguments = vec![OsString::from("run"), OsString::from_vec(vec![b'a', 0xff])];
        let transformed = Arguments::transform(arguments.clone()).expect("inactive argv passes");
        let (forwarded, pending, profile) = transformed.into_parts();
        assert_eq!(forwarded.as_ref(), arguments);
        assert!(pending.is_none());
        assert!(profile.is_none());
    }

    #[test]
    fn exact_prefix_strips_only_three_tokens_and_preserves_opaque_run_arguments() {
        let opaque = OsString::from_vec(vec![b'.', b'/', 0xff]);
        let launch = "01".repeat(32);
        let transformed = Arguments::transform([
            OsString::from("--baseline-evidence-v1"),
            OsString::from("/tmp/evidence.sock"),
            OsString::from(launch),
            OsString::from("run"),
            opaque.clone(),
        ])
        .expect("exact prefix validates");
        let (forwarded, pending, profile) = transformed.into_parts();
        assert_eq!(forwarded.as_ref(), [OsString::from("run"), opaque]);
        assert!(pending.is_some());
        assert!(profile.is_none());
    }

    #[test]
    fn active_and_inactive_forwarding_produce_the_same_sole_typed_command() {
        let ordinary = vec![OsString::from("run"), OsString::from("./fixture")];
        let inactive = Arguments::transform(ordinary.clone()).expect("inactive argv validates");
        let launch = "01".repeat(32);
        let active = Arguments::transform([
            OsString::from("--baseline-evidence-v1"),
            OsString::from("/tmp/evidence.sock"),
            OsString::from(launch),
            ordinary[0].clone(),
            ordinary[1].clone(),
        ])
        .expect("active argv validates");
        let (inactive, inactive_pending, inactive_profile) = inactive.into_parts();
        let (active, active_pending, active_profile) = active.into_parts();
        assert!(inactive_pending.is_none());
        assert!(active_pending.is_some());
        assert!(inactive_profile.is_none());
        assert!(active_profile.is_none());
        assert_eq!(inactive, active);
        assert_eq!(
            crate::command::parse(inactive),
            crate::command::parse(active)
        );
    }

    #[test]
    fn malformed_or_non_run_prefix_fails_closed() {
        let launch = "01".repeat(32);
        for arguments in [
            vec![OsString::from("--baseline-evidence-v1")],
            vec![
                OsString::from("--baseline-evidence-v1"),
                OsString::from("/tmp/evidence.sock"),
                OsString::from(&launch),
                OsString::from("check"),
            ],
        ] {
            assert!(Arguments::transform(arguments).is_err());
        }
    }

    #[test]
    fn profile_prefix_is_closed_typed_and_preserves_the_remaining_arguments() {
        for (name, expected) in [
            ("baseline", rawr_iced::profile::Fixture::Baseline),
            ("variant", rawr_iced::profile::Fixture::Variant),
        ] {
            let transformed = Arguments::transform([
                OsString::from("--baseline-profile-v1"),
                OsString::from(name),
                OsString::from("check"),
                OsString::from("./fixture"),
            ])
            .expect("closed profile prefix validates");
            let (forwarded, evidence, profile) = transformed.into_parts();
            assert_eq!(
                forwarded.as_ref(),
                [OsString::from("check"), OsString::from("./fixture")]
            );
            assert!(evidence.is_none());
            assert_eq!(profile, Some(expected));
        }
        assert!(Arguments::transform([
            OsString::from("--baseline-profile-v1"),
            OsString::from("unknown"),
            OsString::from("check"),
        ])
        .is_err());
    }

    #[test]
    fn profile_selection_requires_one_imported_owner_request() {
        let profile = Some(rawr_iced::profile::Fixture::Variant);
        for request in [
            crate::command::Request::Help,
            crate::command::Request::Version,
            crate::command::Request::Execute(crate::command::Command::New {
                path: "fixture".into(),
            }),
            crate::command::Request::Execute(crate::command::Command::Doctor { path: None }),
        ] {
            assert!(super::authorize_request(profile, &request).is_err());
        }
        assert_eq!(
            super::authorize_request(
                profile,
                &crate::command::Request::Execute(crate::command::Command::Check { path: None })
            )
            .expect("an imported check owner accepts the private fixture"),
            profile
        );
    }
}

//! Capability-free facts for one reached config filename acquisition.
//!
//! Construction checks the source branch, not the producer or current source.
//! No descriptor, warning-output success, or resource failure is represented.

/// The two filename families reached by pinned get_file_path.
#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub(in crate::input::membership) enum Filename {
    Config,
    Credentials,
}

impl Filename {
    pub(in crate::input::membership) fn legacy(self) -> &'static str {
        match self {
            Self::Config => "config",
            Self::Credentials => "credentials",
        }
    }

    pub(in crate::input::membership) fn toml(self) -> &'static str {
        match self {
            Self::Config => "config.toml",
            Self::Credentials => "credentials.toml",
        }
    }
}

/// same-file 1.0.6 compares only these facts from successful descriptor fstat.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::input::membership) struct Identity {
    pub(in crate::input::membership) device: u64,
    pub(in crate::input::membership) inode: u64,
}

/// InvalidInput is std's interior-NUL conversion error, not an invented errno.
/// Missing observations and admission failures must never enter this enum.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::input::membership) enum NativeError {
    InvalidInput,
    Os(i32),
}

impl NativeError {
    fn valid(self) -> bool {
        match self {
            Self::InvalidInput => true,
            Self::Os(code) => code > 0,
        }
    }
}

/// A failed fstat still means open succeeded; live cleanup must close that file.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::input::membership) enum Attempt {
    Acquired(Identity),
    OpenFailed(NativeError),
    StatFailed(i32),
}

impl Attempt {
    fn valid(self) -> bool {
        match self {
            Self::Acquired(_) => true,
            // Pinned std's OpenOptions uses cvt_r(open), so interruption is
            // retried internally and cannot be its completed native failure.
            Self::OpenFailed(NativeError::Os(code)) if code == nix::libc::EINTR => false,
            Self::OpenFailed(error) => error.valid(),
            // Descriptor fstat does not perform native pathname conversion.
            Self::StatFailed(code) => code > 0,
        }
    }
}

/// Only reached after legacy acquisition failed. Any metadata kind is present.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::input::membership) enum Exists {
    Present,
    Failed(NativeError),
}

/// Exactly one of these conditional branches follows legacy acquisition.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::input::membership) enum After {
    None,
    Second(Attempt),
    Fallback(Exists),
}

/// Selected spelling in the physical-source projection. Full Cargo returns
/// that spelling only after any reached terminal warning completes successfully.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::input::membership) enum Selection {
    Legacy,
    Toml,
    None,
}

/// Diagnostic intent retained across the omitted terminal stage, never
/// evidence of rendering or output success.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::input::membership) enum Diagnostic {
    None,
    Both,
    Deprecated,
}

/// A checked branch transcript (`proof-bundle`), not current membership proof.
/// The enclosing observation must bind its directory and original account.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::input::membership) struct Transcript {
    filename: Filename,
    warn: bool,
    legacy: Attempt,
    after: After,
}

impl Transcript {
    pub(in crate::input::membership) fn checked(
        filename: Filename,
        warn: bool,
        legacy: Attempt,
        after: After,
    ) -> Option<Self> {
        if !legacy.valid() {
            return None;
        }
        let reachable = match (legacy, warn, after) {
            (Attempt::Acquired(_), false, After::None) => true,
            (
                Attempt::Acquired(_),
                true,
                After::Second(Attempt::OpenFailed(NativeError::InvalidInput)),
            ) => false,
            (Attempt::Acquired(_), true, After::Second(second)) => second.valid(),
            // Both fixed suffixes are NUL-free and share the same directory.
            (
                Attempt::OpenFailed(NativeError::InvalidInput),
                _,
                After::Fallback(Exists::Failed(NativeError::InvalidInput)),
            ) => true,
            (
                Attempt::OpenFailed(NativeError::Os(_)) | Attempt::StatFailed(_),
                _,
                After::Fallback(exists),
            ) => match exists {
                Exists::Present => true,
                Exists::Failed(NativeError::Os(code)) => code > 0,
                Exists::Failed(NativeError::InvalidInput) => false,
            },
            _ => false,
        };
        reachable.then_some(Self {
            filename,
            warn,
            legacy,
            after,
        })
    }

    pub(in crate::input::membership) fn filename(self) -> Filename {
        self.filename
    }

    pub(in crate::input::membership) fn warn(self) -> bool {
        self.warn
    }

    pub(in crate::input::membership) fn legacy(self) -> Attempt {
        self.legacy
    }

    pub(in crate::input::membership) fn after(self) -> After {
        self.after
    }

    pub(in crate::input::membership) fn selection(self) -> Selection {
        match self.legacy {
            Attempt::Acquired(_) => Selection::Legacy,
            Attempt::OpenFailed(_) | Attempt::StatFailed(_) => match self.after {
                After::Fallback(Exists::Present) => Selection::Toml,
                _ => Selection::None,
            },
        }
    }

    pub(in crate::input::membership) fn diagnostic(self) -> Diagnostic {
        match (self.legacy, self.after) {
            (Attempt::Acquired(first), After::Second(Attempt::Acquired(second))) => {
                if first == second {
                    Diagnostic::None
                } else {
                    Diagnostic::Both
                }
            }
            (Attempt::Acquired(_), After::Second(_)) => Diagnostic::Deprecated,
            _ => Diagnostic::None,
        }
    }
}

#[cfg(test)]
mod tests;

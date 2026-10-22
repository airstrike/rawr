//! Pinned Cargo color selection for its piped stdout, over supplied config data.
//!
//! Inputs are the eventual closed configuration evaluator's result, not proof
//! of current configuration or full GlobalContext::configure success. The
//! semantic environment projection deliberately cannot supply these inputs.

use super::super::{host, Error, Result};

#[cfg(test)]
mod tests;

/// Exact result shape of get::<TermConfig>("term").unwrap_or_default().
pub(in crate::input::membership) enum Term<'input> {
    Loaded {
        color: Option<&'input [u8]>,
    },
    /// The whole TermConfig deserialization failed at the pinned call site.
    Defaulted,
}

/// Caller-owned CLI and merged TermConfig data, pending complete host binding.
pub(in crate::input::membership) struct Inputs<'input> {
    pub(in crate::input::membership) cli: Option<&'input [u8]>,
    pub(in crate::input::membership) term: Term<'input>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Choice {
    Auto,
    Always,
    Never,
}

/// A rendering decision only; no source, configuration or process authority.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::input::membership) struct Mode {
    choice: Choice,
    strip: bool,
}

impl Mode {
    pub(in crate::input::membership) fn from_inputs(
        input: Inputs<'_>,
        host: &mut host::Host<'_, '_>,
    ) -> Result<Self> {
        let term = match input.term {
            Term::Loaded { color } => color,
            Term::Defaulted => None,
        };
        let choice = match input.cli.or(term) {
            None | Some(b"auto") => Choice::Auto,
            Some(b"always") => Choice::Always,
            Some(b"never") => Choice::Never,
            Some(_) => return Err(Error::Encoding("invalid Cargo color choice")),
        };
        let strip = match choice {
            Choice::Always => false,
            Choice::Never => true,
            Choice::Auto => {
                // anstream reads CLICOLOR before NO_COLOR or CLICOLOR_FORCE.
                // Cargo has a fresh process-global Auto default. Its launcher
                // pipes stdout, so the later TERM/CI/terminal branch cannot run.
                // Preserve the CLICOLOR read even though either non-forced
                // branch ultimately chooses Never for this nonterminal stream.
                let _clicolor_disabled = host.environment(b"CLICOLOR")? == Some(b"0");
                if nonempty(host.environment(b"NO_COLOR")?) {
                    true
                } else {
                    !nonempty(host.environment(b"CLICOLOR_FORCE")?)
                }
            }
        };
        Ok(Self { choice, strip })
    }

    pub(in crate::input::membership) fn strips(self) -> bool {
        self.strip
    }
}

fn nonempty(value: Option<&[u8]>) -> bool {
    value.is_some_and(|value| !value.is_empty())
}

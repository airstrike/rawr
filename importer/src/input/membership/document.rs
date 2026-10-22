//! Complete recorded file bytes become semantic data under the same account.
//!
//! This boundary retains neither live host capability nor package authority.
//! The caller must still establish the producer, selection, and current proof.

use std::alloc::Layout;

use rawr_toml::document;

use super::{host, Budget, Error, Result};
use crate::cargo;

#[cfg(test)]
mod tests;

pub(super) fn parse<'image>(
    file: host::File<'image>,
    limits: &cargo::Limits,
    budget: &mut Budget<'_>,
) -> Result<document::Document<'image>> {
    let bytes = file.bytes();
    // The manifest size is an accounting target recorded on the original
    // input account; the bytes were already retained under the host's physical
    // read bound, so nothing is refused here.
    budget.allocation.exceeded(
        "membership manifest bytes",
        limits.manifest_bytes,
        bytes.len(),
    );
    let source = std::str::from_utf8(bytes)?;
    parse_text(source, limits, budget)
}

pub(super) fn parse_text<'source>(
    source: &'source str,
    limits: &cargo::Limits,
    budget: &mut Budget<'_>,
) -> Result<document::Document<'source>> {
    budget.allocation.exceeded(
        "membership manifest bytes",
        limits.manifest_bytes,
        source.len(),
    );
    // `depth` bounds parser recursion and stays a physical refusal.
    let depth = u32::try_from(limits.depth).unwrap_or(u32::MAX);
    document::parse(source, depth, budget).map_err(|error| match error {
        document::Error::Admission(error) => error,
        document::Error::Allocation => Error::DocumentAllocation,
        document::Error::Overflow => Error::DocumentOverflow,
        document::Error::Invalid(error) => Error::Document(error),
    })
}

impl document::Admission for Budget<'_> {
    type Error = Error;

    fn admit(&mut self, request: document::Request) -> Result<()> {
        match request {
            document::Request::Items(amount) => self.allocation.items(amount)?,
            // A decoded string crossing the string target is recorded;
            // its bytes are admitted through the checked allocation charge.
            document::Request::String(length) => self
                .allocation
                .string_exceeded("membership decoded TOML string bytes", length),
            document::Request::Allocation { bytes, alignment } => {
                Layout::from_size_align(bytes, alignment).map_err(|_| Error::DocumentOverflow)?;
                // Charge the full requested replacement layout before each
                // allocation. Old layouts remain spent in the original account.
                self.allocation.bytes(bytes)?;
            }
        }
        Ok(())
    }
}

// A loader Session can own the observation host and its original account
// together. This delegates ownership accounting; it creates no second budget.
impl document::Admission for host::Host<'_, '_> {
    type Error = Error;

    fn admit(&mut self, request: document::Request) -> Result<()> {
        self.budget().admit(request)
    }
}

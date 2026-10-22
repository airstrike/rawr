//! Pinned Human output rendering and comparison, without interpreting lines.
//!
//! Cargo emits every archive `rel_str` followed by LF, including names that
//! already contain LF. This encoding is not injective. A comparison establishes
//! byte agreement with supplied projection data, never membership authority.
//! One exact anstream parser state persists across all filename and LF writes.
//! Effective mode inputs still require the complete closed config/command join.

use super::{archive, Error, Result};

pub(super) mod mode;

#[cfg(test)]
mod tests;

/// An admitted rendering view; no second owned output or filename model.
pub(super) struct Expected<'projection> {
    projection: &'projection archive::Projection,
    mode: mode::Mode,
    length: usize,
}

/// Byte agreement only. Full selector and cold ownership binding are separate.
#[derive(Debug)]
pub(super) struct Compared<'projection, 'output> {
    projection: &'projection archive::Projection,
    mode: mode::Mode,
    output: &'output [u8],
}

#[derive(Debug, Eq, PartialEq, thiserror::Error)]
pub(super) enum Mismatch {
    #[error("expected {expected} complete bytes, received {actual}")]
    Length { expected: usize, actual: usize },
    #[error("different byte at offset {offset}")]
    Byte { offset: usize },
}

impl<'projection> Expected<'projection> {
    /// `output_limit` is physical: the retained `cargo package` stdout is
    /// captured under `process::CAPTURE_BYTES`, and an expected
    /// rendering longer than the captured bound can never match, so it is
    /// refused rather than recorded. It is never an accounting target.
    pub(super) fn from_projection(
        projection: &'projection archive::Projection,
        mode: mode::Mode,
        output_limit: usize,
    ) -> Result<Self> {
        // The row/name ownership was admitted before projection. This pass is
        // bounded by that envelope and admits the complete rendering length
        // before any actual-output comparison. It performs no allocation, IO,
        // candidate decoding, or path/artifact check to charge a second time.
        let mut length = 0usize;
        render(projection, mode, |bytes| {
            length = length
                .checked_add(bytes.len())
                .ok_or(Error::Encoding("human package output length overflow"))?;
            super::super::limited("Cargo package stdout bytes", output_limit, length)?;
            Ok(())
        })?;
        Ok(Self {
            projection,
            mode,
            length,
        })
    }

    pub(super) fn compare<'output>(
        self,
        actual: &'output [u8],
    ) -> Result<Compared<'projection, 'output>> {
        if actual.len() != self.length {
            return Err(Mismatch::Length {
                expected: self.length,
                actual: actual.len(),
            }
            .into());
        }
        let mut offset = 0;
        render(self.projection, self.mode, |bytes| {
            for &expected in bytes {
                if actual[offset] != expected {
                    return Err(Mismatch::Byte { offset }.into());
                }
                offset += 1;
            }
            Ok(())
        })?;
        Ok(Compared {
            projection: self.projection,
            mode: self.mode,
            output: actual,
        })
    }
}

impl Compared<'_, '_> {
    pub(super) fn projection(&self) -> &archive::Projection {
        self.projection
    }

    pub(super) fn output(&self) -> &[u8] {
        self.output
    }

    pub(super) fn mode(&self) -> mode::Mode {
        self.mode
    }
}

fn render(
    projection: &archive::Projection,
    mode: mode::Mode,
    mut accept: impl FnMut(&[u8]) -> Result<()>,
) -> Result<()> {
    // StripBytes and its UTF-8 parser own only fixed scalar state. Returned
    // slices borrow the already admitted row or the static separator. No
    // AutoStream, environment/terminal query, owned helper, or IO is reachable.
    let mut strip = anstream::adapter::StripBytes::new();
    for row in projection.rows() {
        // Cargo's macro writes the complete string, then separately writes LF.
        // Exhaust each iterator so controls at a chunk boundary update state.
        for chunk in [row.archive(), b"\n".as_slice()] {
            if mode.strips() {
                for bytes in strip.strip_next(chunk) {
                    accept(bytes)?;
                }
            } else {
                accept(chunk)?;
            }
        }
    }
    Ok(())
}

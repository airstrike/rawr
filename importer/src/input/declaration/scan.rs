//! Borrowed emission of the pinned saved-output grammar.
//!
//! The scanner imposes no size or count ceilings. Accounting targets
//! (`string_bytes`) are never consulted here: each caller measures every emitted payload against
//! its own account, which records a crossing and continues.

/// One ordered observation; the payload borrows the supplied file bytes.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::input) enum Event<'bytes> {
    Directive(&'bytes str),
    Path(&'bytes str),
    Environment(&'bytes str),
}

/// Grammar failures retain only borrowed text or fixed diagnostic fields.
#[derive(Debug, thiserror::Error)]
pub(in crate::input) enum Error<'bytes> {
    #[error("saved build output does not end at a line boundary")]
    Truncated,
    #[error("saved output is not UTF-8: {0}")]
    Utf8(#[source] std::str::Utf8Error),
    #[error("malformed saved output: {0}")]
    Malformed(&'bytes str),
}

/// Visit without owning payloads or consulting filesystem/process state.
///
/// Completion and UTF-8 are checked before the first event. An instruction is
/// emitted before its rerun-specific validation, matching the legacy collector's
/// storage/error order. Callback failure prevents all subsequent observations.
pub(in crate::input) fn visit<'bytes, E>(
    bytes: &'bytes [u8],
    mut emit: impl FnMut(Event<'bytes>) -> Result<(), E>,
) -> Result<(), E>
where
    E: From<Error<'bytes>>,
{
    if !bytes.is_empty() && !bytes.ends_with(b"\n") {
        return Err(Error::Truncated.into());
    }
    let text = std::str::from_utf8(bytes).map_err(Error::Utf8)?;
    for line in text.split_terminator('\n') {
        if let Some(instruction) = instruction(line)? {
            emit(Event::Directive(instruction))?;
            if let Some(path) = instruction.strip_prefix("rerun-if-changed=") {
                if path.is_empty() {
                    return Err(Error::Malformed("empty rerun path").into());
                }
                emit(Event::Path(path))?;
            } else if let Some(key) = instruction.strip_prefix("rerun-if-env-changed=") {
                if key.is_empty() || key.contains('=') || key.contains('\0') {
                    return Err(Error::Malformed("invalid rerun environment key").into());
                }
                emit(Event::Environment(key))?;
            }
        }
    }
    Ok(())
}

fn instruction(line: &str) -> Result<Option<&str>, Error<'_>> {
    let value = line
        .strip_prefix("cargo::")
        .or_else(|| line.strip_prefix("cargo:"));
    let Some(value) = value else {
        return Ok(None);
    };
    if value.is_empty() || !value.contains('=') {
        return Err(Error::Malformed(line));
    }
    Ok(Some(value))
}

//! Original-account canonical acquisition data; current admission is unconnected.

use std::ffi::CStr;

use crate::input::membership::{observed, Budget, Error, Result};

/// Private deterministic seam. Returned bytes borrow only the transfer buffer;
/// this interface cannot supply a record, alternate budget or source authority.
pub(super) trait System {
    fn canonicalize<'buffer>(
        &mut self,
        path: &CStr,
        buffer: &'buffer mut rawr_canonical::Buffer,
    ) -> std::result::Result<&'buffer CStr, rawr_canonical::Error>;
}

pub(super) fn acquire(
    path: &[u8],
    budget: &mut Budget<'_>,
    system: &mut impl System,
) -> Result<observed::Record> {
    let path = observed::Path::from_bytes(path, budget)?;
    let length = path
        .bytes()
        .len()
        .checked_add(1)
        .ok_or(Error::Encoding("canonical native path length overflow"))?;
    budget.allocation.items(1)?;
    budget.allocation.bytes(length)?;
    let mut native = Vec::new();
    native.try_reserve_exact(length)?;
    native.extend_from_slice(path.bytes());
    native.push(0);
    let native = CStr::from_bytes_with_nul(&native)
        .map_err(|_| Error::Encoding("invalid canonical native path"))?;

    // As with directory transfer/file scratch, this is fixed stack storage,
    // not a hidden heap allocation or an input-length restriction.
    let mut buffer = [0; rawr_canonical::BUFFER_BYTES];
    budget.graph.paths(1)?;
    let value = match system.canonicalize(native, &mut buffer) {
        Ok(value) => Ok(observed::Path::from_bytes(value.to_bytes(), budget)?),
        Err(rawr_canonical::Error::Native(error)) => {
            let code = error
                .raw_os_error()
                .filter(|code| *code > 0)
                .ok_or(Error::Encoding("canonical native error must be positive"))?;
            Err(code)
        }
        Err(rawr_canonical::Error::Framing(rawr_canonical::Framing::Pointer)) => {
            return Err(Error::Encoding("canonical native pointer mismatch"));
        }
        Err(rawr_canonical::Error::Framing(rawr_canonical::Framing::Termination)) => {
            return Err(Error::Encoding("canonical native terminator missing"));
        }
        Err(rawr_canonical::Error::Unsupported) => {
            return Err(Error::Encoding(
                "canonical native ABI unqualified on target",
            ));
        }
    };
    observed::Record {
        query: observed::Query::Canonical(path),
        value: observed::Value::Canonical(value),
    }
    .checked()
}

#[cfg(test)]
mod tests;

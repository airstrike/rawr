//! Native directory observation under the original graph work authority.

use std::ffi::{CStr, OsString};
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::{Path, PathBuf};

use crate::node::{Error, GraphBudget};

struct Host<'budget> {
    budget: &'budget GraphBudget,
    visible_limit: usize,
}

impl rawr_directory::Admission for Host<'_> {
    type Error = Error;

    fn admit(&mut self, work: rawr_directory::Work) -> Result<(), Error> {
        use rawr_directory::Work;

        match work {
            Work::Reopen { .. } => self.budget.paths(2), // open and prospective cleanup
            Work::Open { .. }
            | Work::Mount
            | Work::Read { .. }
            | Work::Record
            | Work::EntryType { .. } => {
                // These are actual directory/entry probes, including failed and
                // terminal observations. No output name or file kind is trusted
                // merely because this work was admitted.
                self.budget.paths(1)
            }
            Work::Allocate { bytes, .. } => self.budget.decode(bytes),
            Work::Visible { entries } if entries > self.visible_limit => Err(Error::Limit {
                resource: "runner bundle directory members",
                limit: self.visible_limit,
                actual: entries,
            }),
            Work::Visible { .. } => Ok(()),
            Work::Name { .. }
            | Work::Retain { .. }
            | Work::Copy { .. }
            | Work::Order
            | Work::Compare { .. } => {
                // Pure work is bounded by the admitted raw observations and
                // requested ownership: one fixed 8192-byte transfer per Read,
                // minimum 24-byte records, names <=1023 bytes, and an in-place
                // O(n log n) sort over retained records. Retain is temporary raw
                // ownership, never logical member cardinality. These events
                // neither allocate nor perform filesystem/candidate comparisons.
                Ok(())
            }
        }
    }
}

pub(super) fn names(
    path: &Path,
    limit: usize,
    budget: &GraphBudget,
) -> Result<Vec<OsString>, Error> {
    observe(path, limit, budget, |path, buffer, host| {
        rawr_directory::collect(path, buffer, host)
    })
}

fn observe(
    path: &Path,
    limit: usize,
    budget: &GraphBudget,
    collect: impl FnOnce(
        &CStr,
        &mut rawr_directory::Buffer,
        &mut Host<'_>,
    ) -> Result<rawr_directory::Names, rawr_directory::Error<Error>>,
) -> Result<Vec<OsString>, Error> {
    let (path_bytes, error_path) = caller(path, budget)?;
    // Borrow the already admitted terminated vector. CString conversions can
    // hide a boxed-slice capacity adjustment; no such conversion is necessary.
    let c_path = CStr::from_bytes_with_nul(&path_bytes)
        .map_err(|_| Error::State("bounded directory path contains an interior NUL"))?;
    let mut host = Host {
        budget,
        visible_limit: limit,
    };
    let mut buffer = [0; rawr_directory::BUFFER_BYTES];
    let current = collect(c_path, &mut buffer, &mut host)
        .and_then(|names| names.sorted(&mut host))
        .map_err(|error| convert(error, error_path))?;
    let mut names = output(current.len(), budget)?;
    for name in current {
        // Both vectors have admitted storage. This transfers the existing name
        // byte allocation; OsString::from_vec does not copy or reallocate it.
        names.push(OsString::from_vec(name.into_bytes()));
    }
    Ok(names)
}

fn caller(path: &Path, budget: &GraphBudget) -> Result<(Vec<u8>, PathBuf), Error> {
    let bytes = path.as_os_str().as_bytes();
    let terminated = bytes.len().checked_add(1).ok_or(Error::Allocation)?;
    let owned = std::mem::size_of::<PathBuf>()
        .checked_add(bytes.len())
        .and_then(|size| size.checked_add(terminated))
        .ok_or(Error::Allocation)?;
    budget.decode(owned)?;
    let mut error_path = PathBuf::new();
    error_path
        .try_reserve_exact(bytes.len())
        .map_err(|_| Error::Allocation)?;
    error_path.push(path);
    let mut native = Vec::new();
    native
        .try_reserve_exact(terminated)
        .map_err(|_| Error::Allocation)?;
    native.extend_from_slice(bytes);
    native.push(0);
    Ok((native, error_path))
}

fn output(length: usize, budget: &GraphBudget) -> Result<Vec<OsString>, Error> {
    budget.decode(
        length
            .checked_mul(std::mem::size_of::<OsString>())
            .ok_or(Error::Allocation)?,
    )?;
    let mut names = Vec::new();
    names
        .try_reserve_exact(length)
        .map_err(|_| Error::Allocation)?;
    Ok(names)
}

fn convert(error: rawr_directory::Error<Error>, path: PathBuf) -> Error {
    match error {
        rawr_directory::Error::Admission(error) => error,
        rawr_directory::Error::Io { operation, source } => Error::Io {
            operation,
            path,
            source,
        },
        rawr_directory::Error::Framing(_) => Error::State("native directory framing is malformed"),
        rawr_directory::Error::Allocation => Error::Allocation,
        rawr_directory::Error::Terminal => Error::State("native directory reader is terminal"),
    }
}

#[cfg(test)]
mod tests;

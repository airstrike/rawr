//! Native directory observation under the original graph work authority.

use std::ffi::{CStr, OsString};
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::{Path, PathBuf};

use crate::node::Error;

struct Host {
    visible_limit: usize,
}

impl rawr_directory::Admission for Host {
    type Error = Error;

    fn admit(&mut self, work: rawr_directory::Work) -> Result<(), Error> {
        use rawr_directory::Work;

        match work {
            Work::Visible { entries } if entries > self.visible_limit => Err(Error::Limit {
                resource: "runner bundle directory members",
                limit: self.visible_limit,
                actual: entries,
            }),
            // Probes, allocations and pure work are bounded by the raw
            // observations themselves; only the member count is a limit here.
            _ => Ok(()),
        }
    }
}

pub(super) fn names(path: &Path, limit: usize) -> Result<Vec<OsString>, Error> {
    observe(path, limit, |path, buffer, host| {
        rawr_directory::collect(path, buffer, host)
    })
}

fn observe(
    path: &Path,
    limit: usize,
    collect: impl FnOnce(
        &CStr,
        &mut rawr_directory::Buffer,
        &mut Host,
    ) -> Result<rawr_directory::Names, rawr_directory::Error<Error>>,
) -> Result<Vec<OsString>, Error> {
    let (path_bytes, error_path) = caller(path)?;
    // Borrow the already admitted terminated vector. CString conversions can
    // hide a boxed-slice capacity adjustment; no such conversion is necessary.
    let c_path = CStr::from_bytes_with_nul(&path_bytes)
        .map_err(|_| Error::State("bounded directory path contains an interior NUL"))?;
    let mut host = Host {
        visible_limit: limit,
    };
    let mut buffer = [0; rawr_directory::BUFFER_BYTES];
    let current = collect(c_path, &mut buffer, &mut host)
        .and_then(|names| names.sorted(&mut host))
        .map_err(|error| convert(error, error_path))?;
    let mut names = output(current.len())?;
    for name in current {
        // Both vectors have admitted storage. This transfers the existing name
        // byte allocation; OsString::from_vec does not copy or reallocate it.
        names.push(OsString::from_vec(name.into_bytes()));
    }
    Ok(names)
}

fn caller(path: &Path) -> Result<(Vec<u8>, PathBuf), Error> {
    let bytes = path.as_os_str().as_bytes();
    let terminated = bytes.len().checked_add(1).ok_or(Error::Allocation)?;
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

fn output(length: usize) -> Result<Vec<OsString>, Error> {
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

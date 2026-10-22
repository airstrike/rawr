use std::ffi::CStr;

use crate::{admit, sys, Admission, Batch, Buffer, Error, Work};

/// An independently owned directory stream. Its descriptor is never exported.
pub struct Reader {
    directory: sys::Directory,
    union: bool,
    terminal: bool,
}

pub enum Step<'buffer> {
    Batch(Batch<'buffer>),
    /// Only a successful syscall returning zero establishes EOF.
    Eof,
}

impl Reader {
    pub fn open<A: Admission>(path: &CStr, admission: &mut A) -> Result<Self, Error<A::Error>> {
        let (directory, union) =
            open(path, admission, sys::Directory::open, sys::Directory::union)?;
        Ok(Self {
            directory,
            union,
            terminal: false,
        })
    }

    /// Mount policy was observed on this stream before its first read.
    pub const fn is_union(&self) -> bool {
        self.union
    }

    /// Returned entries cannot outlive or alias a mutable refill of `buffer`.
    /// The optional kernel EOF tail is ignored: on union mounts another call
    /// may traverse a lower layer after an upper layer's positive final batch.
    ///
    /// ```compile_fail
    /// use rawr_directory::{Admission, Buffer, Reader, Step};
    /// fn refill<A: Admission>(reader: &mut Reader, bytes: &mut Buffer, host: &mut A) {
    ///     if let Ok(Step::Batch(mut batch)) = reader.read(bytes, host) {
    ///         let entry = batch.next(host);
    ///         let _ = reader.read(bytes, host);
    ///         drop(entry);
    ///     }
    /// }
    /// ```
    pub fn read<'buffer, A: Admission>(
        &mut self,
        buffer: &'buffer mut Buffer,
        admission: &mut A,
    ) -> Result<Step<'buffer>, Error<A::Error>> {
        read(&mut self.terminal, buffer, admission, |buffer| {
            self.directory.read(buffer)
        })
    }
}

// Both real and fixture reads use this exact admission/EOF/error transition.
pub(super) fn read<'buffer, A: Admission>(
    terminal: &mut bool,
    buffer: &'buffer mut Buffer,
    admission: &mut A,
    operation: impl FnOnce(&mut Buffer) -> std::io::Result<usize>,
) -> Result<Step<'buffer>, Error<A::Error>> {
    if *terminal {
        return Err(Error::Terminal);
    }
    *terminal = true;
    admit(
        admission,
        Work::Read {
            buffer_bytes: buffer.len(),
        },
    )?;
    match operation(buffer) {
        Ok(0) => Ok(Step::Eof),
        Ok(bytes) => {
            // The kernel may reserve a four-byte status tail. Accept only the
            // returned prefix, never the initialized stale remainder or flags.
            let prefix = buffer.get(..bytes).ok_or(crate::Framing::Prefix)?;
            *terminal = false;
            Ok(Step::Batch(Batch::new(prefix)))
        }
        Err(source) => Err(Error::Io {
            operation: "read native directory batch",
            source,
        }),
    }
}

fn open<A: Admission, D>(
    path: &CStr,
    admission: &mut A,
    open: impl FnOnce(&CStr) -> std::io::Result<D>,
    mount: impl FnOnce(&D) -> std::io::Result<bool>,
) -> Result<(D, bool), Error<A::Error>> {
    admit(
        admission,
        Work::Open {
            path_bytes: path.to_bytes_with_nul().len(),
        },
    )?;
    let directory = open(path).map_err(|source| Error::Io {
        operation: "open independent directory",
        source,
    })?;
    admit(admission, Work::Mount)?;
    let union = mount(&directory).map_err(|source| Error::Io {
        operation: "inspect directory union mount",
        source,
    })?;
    Ok((directory, union))
}

#[cfg(test)]
mod tests;

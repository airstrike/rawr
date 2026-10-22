//! Private directory replay foundation.
//!
//! The raw reader owns an independently opened descriptor. Darwin can retarget
//! that descriptor while traversing a union mount, so it never exports lookup
//! authority or accepts a duplicated caller descriptor. Its transfer storage is
//! one initialized, caller-owned [`Buffer`]; batches borrow only the returned
//! prefix. No libc `DIR`, directory iterator, or hidden collection is used.
//!
//! [`Admission`] is a prospective accounting seam, not a budget. Every operation
//! must use the caller's original cumulative authority. Requested heap layouts,
//! byte work, record probes, and system calls are exposed separately. Errors,
//! including interrupted reads, terminate the observation without retrying it.
//! Fixed stack storage, allocator metadata/rounding, and kernel-internal work
//! are distinct from requested user-space heap layouts; see `SAFETY.md`.

#![deny(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

mod collect;
mod frame;
mod reader;
mod stream;
#[allow(unsafe_code)]
mod sys;

pub use collect::{collect, Name, Names};
pub use frame::{Batch, Entry, Framing, Record};
pub use reader::{Reader, Step};
pub use stream::{Entry as StreamEntry, FileType, Mount, Opening, Stream};

/// One fixed transfer buffer, including Darwin's optional four-byte status tail.
pub const BUFFER_BYTES: usize = 8 * 1024;
pub type Buffer = [u8; BUFFER_BYTES];

/// The caller supplies the only resource authority. Refusal precedes the work.
pub trait Admission {
    type Error;

    fn admit(&mut self, work: Work) -> Result<(), Self::Error>;
}

/// Prospective work at the directory boundary. There are no internal allowances.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Work {
    /// An independent open; the supplied C string already belongs to the caller.
    Open { path_bytes: usize },
    /// Reopen this stream's directory through its owned descriptor. The only
    /// spelling is '.', never a caller-supplied path or descriptor.
    Reopen { path_bytes: usize },
    /// One reached unknown-d_type metadata lookup, relative to the associated
    /// directory descriptor and without following the entry's final symlink.
    EntryType { name_bytes: usize },
    /// A fixed-size fstatfs probe on the newly opened descriptor.
    Mount,
    /// One syscall attempt, including attempts returning EINTR, an error, or EOF.
    Read { buffer_bytes: usize },
    /// One framing probe, including the final exhausted-batch probe.
    Record,
    /// Inspect these initialized name bytes, including their terminating NUL.
    Name { bytes: usize },
    /// Admit temporary physical-record ownership before any expansion. This is
    /// work/storage, never the caller's visible-member cardinality limit.
    Retain { entries: usize, name_bytes: usize },
    /// Check logical member cardinality after union collapse, before returning
    /// or transferring the complete result. No separate result is allocated.
    Visible { entries: usize },
    /// Request this complete new heap layout before a fallible exact reservation.
    /// The previous allocation remains admitted until replacement succeeds.
    Allocate {
        storage: Storage,
        bytes: usize,
        alignment: usize,
    },
    /// At most these bytes are copied, including old storage during growth.
    Copy { bytes: usize },
    /// One bounded sorting or collapse step, without heap allocation.
    Order,
    /// Compare native names, examining at most this many bytes in each name.
    Compare { bytes: usize },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Storage {
    Entries,
    Name,
}

#[derive(Debug, thiserror::Error)]
pub enum Error<E> {
    #[error("directory work was not admitted")]
    Admission(E),
    #[error("{operation}: {source}")]
    Io {
        operation: &'static str,
        source: std::io::Error,
    },
    #[error("invalid native directory record: {0}")]
    Framing(#[from] Framing),
    #[error("directory storage cannot be represented or allocated")]
    Allocation,
    #[error("directory reader is already terminal")]
    Terminal,
}

fn admit<A: Admission>(admission: &mut A, work: Work) -> Result<(), Error<A::Error>> {
    admission.admit(work).map_err(Error::Admission)
}

#[cfg(test)]
mod tests;

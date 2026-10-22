use std::cmp::Ordering;
use std::ffi::CStr;

use crate::{admit, Admission, Batch, Buffer, Error, Reader, Record, Step, Storage, Work};

const WHITEOUT: u8 = 14;

/// Names from a complete successful stream, in the pinned reader's native order.
/// In union mode the first nonzero-inode occurrence wins, including a whiteout
/// that suppresses the name. Ordinary directories keep duplicate occurrences.
pub struct Names(Vec<Name>);

/// Native bytes with charged ownership. No UTF-8 conversion is performed.
pub struct Name {
    bytes: Vec<u8>,
    kind: u8,
    ordinal: usize,
    hidden: bool,
}

impl Name {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub const fn kind(&self) -> u8 {
        self.kind
    }

    /// Transfer the already admitted allocation without copying its bytes.
    pub fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }
}

impl Names {
    pub(crate) fn get(&self, index: usize) -> Option<&Name> {
        self.0.get(index)
    }
    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn iter(&self) -> impl ExactSizeIterator<Item = &Name> {
        self.0.iter()
    }

    /// Canonical native byte order, with every comparison/move admitted and no
    /// sorting allocation. Failure consumes the incomplete result.
    pub fn sorted<A: Admission>(mut self, admission: &mut A) -> Result<Self, Error<A::Error>> {
        sort(&mut self.0, Order::Name, admission)?;
        Ok(self)
    }
}

impl IntoIterator for Names {
    type Item = Name;
    type IntoIter = std::vec::IntoIter<Name>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

/// Open and completely enumerate a fresh independent stream. A caller cannot
/// turn a previously consumed raw reader into a complete `Names` observation.
/// A failed read, parser, allocation, or admission yields no partial result.
pub fn collect<A: Admission>(
    path: &CStr,
    buffer: &mut Buffer,
    admission: &mut A,
) -> Result<Names, Error<A::Error>> {
    let mut reader = Reader::open(path, admission)?;
    run(&mut reader, buffer, admission)
}

pub(crate) fn run<A: Admission>(
    reader: &mut impl Source,
    buffer: &mut Buffer,
    admission: &mut A,
) -> Result<Names, Error<A::Error>> {
    let mut collector = Collector::new(reader.is_union());
    loop {
        match reader.read(buffer, admission)? {
            Step::Batch(mut batch) => collector.batch(&mut batch, admission)?,
            Step::Eof => return collector.finish(admission),
        }
    }
}

// This private seam keeps synthetic streams on the production collector loop.
pub(crate) trait Source {
    fn is_union(&self) -> bool;
    fn read<'buffer, A: Admission>(
        &mut self,
        buffer: &'buffer mut Buffer,
        admission: &mut A,
    ) -> Result<Step<'buffer>, Error<A::Error>>;
}

impl Source for Reader {
    fn is_union(&self) -> bool {
        self.is_union()
    }

    fn read<'buffer, A: Admission>(
        &mut self,
        buffer: &'buffer mut Buffer,
        admission: &mut A,
    ) -> Result<Step<'buffer>, Error<A::Error>> {
        self.read(buffer, admission)
    }
}

struct Collector {
    entries: Vec<Name>,
    // Track admitted slots independently of any allocator-provided excess.
    capacity: usize,
    union: bool,
}

impl Collector {
    const fn new(union: bool) -> Self {
        Self {
            entries: Vec::new(),
            capacity: 0,
            union,
        }
    }

    fn batch<A: Admission>(
        &mut self,
        batch: &mut Batch<'_>,
        admission: &mut A,
    ) -> Result<(), Error<A::Error>> {
        while let Some(record) = batch.next(admission)? {
            let Record::Entry(entry) = record else {
                continue;
            };
            if matches!(entry.name(), b"." | b"..") || (!self.union && entry.kind() == WHITEOUT) {
                continue;
            }
            self.push(entry.name(), entry.kind(), admission)?;
        }
        Ok(())
    }

    fn push<A: Admission>(
        &mut self,
        bytes: &[u8],
        kind: u8,
        admission: &mut A,
    ) -> Result<(), Error<A::Error>> {
        admit(
            admission,
            Work::Retain {
                entries: self.entries.len().checked_add(1).ok_or(Error::Allocation)?,
                name_bytes: bytes.len(),
            },
        )?;
        if self.entries.len() == self.capacity {
            let capacity = self
                .capacity
                .max(1)
                .checked_mul(2)
                .ok_or(Error::Allocation)?;
            let size = capacity
                .checked_mul(std::mem::size_of::<Name>())
                .ok_or(Error::Allocation)?;
            admit(
                admission,
                Work::Allocate {
                    storage: Storage::Entries,
                    bytes: size,
                    alignment: std::mem::align_of::<Name>(),
                },
            )?;
            admit(
                admission,
                Work::Copy {
                    bytes: self
                        .capacity
                        .checked_mul(std::mem::size_of::<Name>())
                        .ok_or(Error::Allocation)?,
                },
            )?;
            self.entries
                .try_reserve_exact(capacity - self.entries.len())
                .map_err(|_| Error::Allocation)?;
            self.capacity = capacity;
        }
        admit(
            admission,
            Work::Allocate {
                storage: Storage::Name,
                bytes: bytes.len(),
                alignment: std::mem::align_of::<u8>(),
            },
        )?;
        let mut name = Vec::new();
        name.try_reserve_exact(bytes.len())
            .map_err(|_| Error::Allocation)?;
        admit(admission, Work::Copy { bytes: bytes.len() })?;
        name.extend_from_slice(bytes);
        let ordinal = self.entries.len();
        self.entries.push(Name {
            bytes: name,
            kind,
            ordinal,
            hidden: false,
        });
        Ok(())
    }

    fn finish<A: Admission>(mut self, admission: &mut A) -> Result<Names, Error<A::Error>> {
        if self.union {
            // Name+ordinal order reproduces stable name sorting without a
            // pointer table or hidden merge-sort buffer. Inode-zero records were
            // excluded before this grouping, exactly as in pinned _filldir.
            sort(&mut self.entries, Order::Name, admission)?;
            for index in 0..self.entries.len() {
                admit(admission, Work::Order)?;
                let duplicate = index != 0
                    && compare_names(&self.entries[index - 1], &self.entries[index], admission)?
                        == Ordering::Equal;
                self.entries[index].hidden = duplicate || self.entries[index].kind == WHITEOUT;
            }
            sort(&mut self.entries, Order::Ordinal, admission)?;
            let mut visible = 0;
            for index in 0..self.entries.len() {
                admit(admission, Work::Order)?;
                if !self.entries[index].hidden {
                    swap(&mut self.entries, visible, index, admission)?;
                    visible += 1;
                }
            }
            // No shrink/reallocation. Dropping hidden names is bounded by the
            // ownership already admitted while collecting physical records.
            self.entries.truncate(visible);
        }
        admit(
            admission,
            Work::Visible {
                entries: self.entries.len(),
            },
        )?;
        Ok(Names(self.entries))
    }
}

#[derive(Clone, Copy)]
enum Order {
    Name,
    Ordinal,
}

// In-place heapsort has bounded auxiliary storage and no infallible comparator
// callback hiding work after admission failure. Every sift/comparison/swap can
// stop immediately. We retain no partially sorted result on failure.
fn sort<A: Admission>(
    entries: &mut [Name],
    order: Order,
    admission: &mut A,
) -> Result<(), Error<A::Error>> {
    for start in (0..entries.len() / 2).rev() {
        sift(entries, start, order, admission)?;
    }
    for end in (1..entries.len()).rev() {
        swap(entries, 0, end, admission)?;
        sift(&mut entries[..end], 0, order, admission)?;
    }
    Ok(())
}

fn sift<A: Admission>(
    entries: &mut [Name],
    mut root: usize,
    order: Order,
    admission: &mut A,
) -> Result<(), Error<A::Error>> {
    loop {
        admit(admission, Work::Order)?;
        let Some(left) = root.checked_mul(2).and_then(|value| value.checked_add(1)) else {
            return Err(Error::Allocation);
        };
        if left >= entries.len() {
            return Ok(());
        }
        let right = left + 1;
        let child = if right < entries.len()
            && compare(&entries[left], &entries[right], order, admission)? == Ordering::Less
        {
            right
        } else {
            left
        };
        if compare(&entries[root], &entries[child], order, admission)? != Ordering::Less {
            return Ok(());
        }
        swap(entries, root, child, admission)?;
        root = child;
    }
}

fn compare<A: Admission>(
    left: &Name,
    right: &Name,
    order: Order,
    admission: &mut A,
) -> Result<Ordering, Error<A::Error>> {
    admit(admission, Work::Order)?;
    let names = match order {
        Order::Name => compare_names(left, right, admission)?,
        Order::Ordinal => Ordering::Equal,
    };
    Ok(names.then_with(|| left.ordinal.cmp(&right.ordinal)))
}

fn compare_names<A: Admission>(
    left: &Name,
    right: &Name,
    admission: &mut A,
) -> Result<Ordering, Error<A::Error>> {
    admit(
        admission,
        Work::Compare {
            bytes: left.bytes.len().min(right.bytes.len()),
        },
    )?;
    Ok(left.bytes.cmp(&right.bytes))
}

fn swap<A: Admission>(
    entries: &mut [Name],
    left: usize,
    right: usize,
    admission: &mut A,
) -> Result<(), Error<A::Error>> {
    if left != right {
        admit(
            admission,
            Work::Copy {
                bytes: 3 * std::mem::size_of::<Name>(),
            },
        )?;
        entries.swap(left, right);
    }
    Ok(())
}

#[cfg(test)]
mod tests;

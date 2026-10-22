//! Cargo's source-shaped directory stream. See stream/PROVENANCE.md.

use std::{cell::Cell, ffi::CStr, io, ops::Range};

use crate::{
    admit, collect, frame, reader, sys, Admission, Batch, Buffer, Error, Names, Record, Step, Work,
};

/// The actual mount probe, including Cargo's failed-probe ordinary-mode branch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Mount {
    Known(bool),
    Failed(i32),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileType {
    Directory,
    File,
    Symlink,
    Character,
    Block,
    Fifo,
    Socket,
    Other,
}

/// Open/mount observations precede union collection. This intermediate cannot
/// yield an entry; callers may retain mount evidence before finish fails.
pub struct Opening(Core<sys::Directory>);

/// A successfully opened source iterator. Ordinary iteration remains lazy;
/// union collection has completed on an independent enumeration descriptor.
pub struct Stream(Core<sys::Directory>);

/// An entry can use only the lookup descriptor of its own stream. It cannot
/// outlive the stream or a mutable next call, and exposes no descriptor.
pub struct Entry<'stream>(Bound<'stream, sys::Directory>);

impl Opening {
    pub fn open<A: Admission>(path: &CStr, admission: &mut A) -> Result<Self, Error<A::Error>> {
        Core::open(path, admission).map(Self)
    }
    pub fn mount(&self) -> Mount {
        self.0.mount
    }
    pub fn finish<A: Admission>(self, admission: &mut A) -> Result<Stream, Error<A::Error>> {
        self.0.finish(admission).map(Stream)
    }
}
impl Stream {
    pub fn mount(&self) -> Mount {
        self.0.mount
    }
    /// A native error terminates iteration explicitly; it never becomes EOF in
    /// the failing call. Subsequent calls return None, matching pinned ReadDir.
    ///
    /// ```compile_fail
    /// use rawr_directory::{Admission, Stream};
    /// fn refill<A: Admission>(stream: &mut Stream, admission: &mut A) {
    ///     let entry = stream.next(admission).ok().flatten().unwrap();
    ///     let _ = stream.next(admission);
    ///     let _ = entry.file_type(admission);
    /// }
    /// ```
    pub fn next<A: Admission>(
        &mut self,
        admission: &mut A,
    ) -> Result<Option<Entry<'_>>, Error<A::Error>> {
        self.0.next(admission).map(|entry| entry.map(Entry))
    }
}
impl<'stream> Entry<'stream> {
    pub fn name(&self) -> &'stream [u8] {
        self.0.name
    }
    pub fn raw_kind(&self) -> u8 {
        self.0.kind
    }
    /// Exactly one reached type query. Native failure remains data for Cargo's
    /// map_or(false); admission/framing failure cannot masquerade as that data.
    /// Consuming the entry prevents retry after a refused inspection.
    pub fn file_type<A: Admission>(
        self,
        admission: &mut A,
    ) -> Result<Result<FileType, i32>, Error<A::Error>> {
        self.0.file_type(admission)
    }
}

trait System: Sized {
    fn open(path: &CStr) -> io::Result<Self>;
    fn mount(&self) -> io::Result<bool>;
    fn reopen(&self) -> io::Result<Self>;
    fn read(&mut self, buffer: &mut Buffer) -> io::Result<usize>;
    fn entry_type(&self, name: &CStr) -> io::Result<libc::mode_t>;
}
impl System for sys::Directory {
    fn open(path: &CStr) -> io::Result<Self> {
        Self::open_follow(path)
    }
    fn mount(&self) -> io::Result<bool> {
        self.union()
    }
    fn reopen(&self) -> io::Result<Self> {
        self.reopen()
    }
    fn read(&mut self, buffer: &mut Buffer) -> io::Result<usize> {
        self.read(buffer)
    }
    fn entry_type(&self, name: &CStr) -> io::Result<libc::mode_t> {
        self.entry_type(name)
    }
}

struct Core<D> {
    directory: D,
    mount: Mount,
    names: Option<Names>,
    index: usize,
    buffer: Buffer,
    position: usize,
    length: usize,
    terminal: bool,
    ended: bool,
    tail_end: bool,
    refused: Cell<bool>,
}
impl<D: System> Core<D> {
    fn open<A: Admission>(path: &CStr, admission: &mut A) -> Result<Self, Error<A::Error>> {
        admit(
            admission,
            Work::Open {
                path_bytes: path.to_bytes_with_nul().len(),
            },
        )?;
        let directory = D::open(path).map_err(|source| Error::Io {
            operation: "open Cargo directory",
            source,
        })?;
        admit(admission, Work::Mount)?;
        let mount = match directory.mount() {
            Ok(union) => Mount::Known(union),
            Err(source) => Mount::Failed(native(source, "inspect Cargo directory mount")?),
        };
        Ok(Self {
            directory,
            mount,
            names: None,
            index: 0,
            buffer: [0; crate::BUFFER_BYTES],
            position: 0,
            length: 0,
            terminal: false,
            ended: false,
            tail_end: false,
            refused: Cell::new(false),
        })
    }
    fn finish<A: Admission>(mut self, admission: &mut A) -> Result<Self, Error<A::Error>> {
        if self.mount == Mount::Known(true) {
            admit(admission, Work::Reopen { path_bytes: 2 })?;
            let directory = self.directory.reopen().map_err(|source| Error::Io {
                operation: "open independent Cargo union stream",
                source,
            })?;
            let mut source = Enumeration {
                directory,
                terminal: false,
            };
            self.names = Some(collect::run(&mut source, &mut self.buffer, admission)?);
            // Source drops here. Original directory never underwent a union
            // read and continues to supply DirEntry::metadata association.
        }
        Ok(self)
    }
    fn next<A: Admission>(
        &mut self,
        admission: &mut A,
    ) -> Result<Option<Bound<'_, D>>, Error<A::Error>> {
        if self.refused.get() {
            return Err(Error::Terminal);
        }
        if self.ended {
            return Ok(None);
        }
        if self.names.is_some() {
            self.refused.set(true);
            admit(admission, Work::Record)?;
            self.refused.set(false);
            let names = self.names.as_ref().ok_or(Error::Terminal)?;
            let Some(name) = names.get(self.index) else {
                self.ended = true;
                return Ok(None);
            };
            self.index += 1;
            self.ended = false;
            return Ok(Some(Bound {
                directory: &self.directory,
                name: name.bytes(),
                kind: name.kind(),
                refused: &self.refused,
            }));
        }
        // Determine the retained range before borrowing it for the caller, so
        // iteration/refill never coexists with a borrowed returned entry.
        let (range, kind) = match self.advance(admission) {
            Ok(Some(entry)) => entry,
            Ok(None) => {
                self.ended = true;
                return Ok(None);
            }
            Err(error) => {
                if matches!(&error, Error::Io { .. }) {
                    self.ended = true;
                } else {
                    self.refused.set(true);
                }
                return Err(error);
            }
        };
        Ok(Some(Bound {
            directory: &self.directory,
            name: &self.buffer[range],
            kind,
            refused: &self.refused,
        }))
    }
    fn advance<A: Admission>(
        &mut self,
        admission: &mut A,
    ) -> Result<Option<(Range<usize>, u8)>, Error<A::Error>> {
        loop {
            if self.position == self.length {
                if self.tail_end {
                    return Ok(None);
                }
                // Pinned readdir clears the optional status tail before each
                // read; an older kernel may leave these bytes untouched.
                const TAIL: usize = crate::BUFFER_BYTES - 4;
                admit(admission, Work::Copy { bytes: 4 })?;
                self.buffer[TAIL..].fill(0);
                match reader::read(&mut self.terminal, &mut self.buffer, admission, |buffer| {
                    self.directory.read(buffer)
                })? {
                    Step::Batch(batch) => {
                        self.length = batch.remaining();
                        self.position = 0;
                    }
                    Step::Eof => return Ok(None),
                }
                let flags = u32::from_ne_bytes([
                    self.buffer[TAIL],
                    self.buffer[TAIL + 1],
                    self.buffer[TAIL + 2],
                    self.buffer[TAIL + 3],
                ]);
                self.tail_end = self.length <= TAIL && flags & 1 != 0;
            }
            let start = self.position;
            let mut batch = Batch::new(&self.buffer[start..self.length]);
            let record = batch.next(admission)?;
            self.position = self.length - batch.remaining();
            let Some(Record::Entry(entry)) = record else {
                continue;
            };
            if matches!(entry.name(), b"." | b"..") || entry.kind() == 14 {
                continue;
            }
            let begin = start + frame::NAME_OFFSET;
            return Ok(Some((begin..begin + entry.name().len(), entry.kind())));
        }
    }
}

struct Enumeration<D> {
    directory: D,
    terminal: bool,
}
impl<D: System> collect::Source for Enumeration<D> {
    fn is_union(&self) -> bool {
        true
    }
    fn read<'buffer, A: Admission>(
        &mut self,
        buffer: &'buffer mut Buffer,
        admission: &mut A,
    ) -> Result<Step<'buffer>, Error<A::Error>> {
        reader::read(&mut self.terminal, buffer, admission, |buffer| {
            self.directory.read(buffer)
        })
    }
}

struct Bound<'stream, D> {
    directory: &'stream D,
    name: &'stream [u8],
    kind: u8,
    refused: &'stream Cell<bool>,
}
impl<D: System> Bound<'_, D> {
    fn file_type<A: Admission>(
        self,
        admission: &mut A,
    ) -> Result<Result<FileType, i32>, Error<A::Error>> {
        if self.refused.get() {
            return Err(Error::Terminal);
        }
        if let Some(kind) = from_hint(self.kind) {
            return Ok(Ok(kind));
        }
        self.refused.set(true);
        // Native names are at most 1023 bytes, independently of caller limits.
        // This fixed stack copy supplies fstatat's required terminator without
        // retaining a new heap path or granting arbitrary lookup capability.
        let length = self.name.len().checked_add(1).ok_or(Error::Allocation)?;
        admit(admission, Work::Name { bytes: length })?;
        let mut bytes = [0; 1024];
        let output = bytes.get_mut(..length).ok_or(crate::Framing::NameLength)?;
        admit(
            admission,
            Work::Copy {
                bytes: self.name.len(),
            },
        )?;
        output[..self.name.len()].copy_from_slice(self.name);
        let name = CStr::from_bytes_with_nul(output).map_err(|_| crate::Framing::Name)?;
        admit(admission, Work::EntryType { name_bytes: length })?;
        let result = match self.directory.entry_type(name) {
            Ok(mode) => Ok(from_mode(mode)),
            Err(source) => Err(native(source, "inspect Cargo directory entry type")?),
        };
        self.refused.set(false);
        Ok(result)
    }
}
fn native<E>(source: io::Error, operation: &'static str) -> Result<i32, Error<E>> {
    source
        .raw_os_error()
        .filter(|code| *code > 0)
        .ok_or(Error::Io { operation, source })
}
fn from_hint(kind: u8) -> Option<FileType> {
    Some(match kind {
        libc::DT_DIR => FileType::Directory,
        libc::DT_REG => FileType::File,
        libc::DT_LNK => FileType::Symlink,
        libc::DT_CHR => FileType::Character,
        libc::DT_BLK => FileType::Block,
        libc::DT_FIFO => FileType::Fifo,
        libc::DT_SOCK => FileType::Socket,
        _ => return None,
    })
}
fn from_mode(mode: libc::mode_t) -> FileType {
    match mode & libc::S_IFMT {
        libc::S_IFDIR => FileType::Directory,
        libc::S_IFREG => FileType::File,
        libc::S_IFLNK => FileType::Symlink,
        libc::S_IFCHR => FileType::Character,
        libc::S_IFBLK => FileType::Block,
        libc::S_IFIFO => FileType::Fifo,
        libc::S_IFSOCK => FileType::Socket,
        _ => FileType::Other,
    }
}

#[cfg(test)]
mod tests;

//! One consumer's owned lookup directories; no observation survives completion.

use std::ffi::{CStr, CString, OsStr};
use std::fs::{File, Metadata};
use std::io::Read as _;
use std::os::unix::{ffi::OsStrExt as _, fs::MetadataExt as _};
use std::path::{Component, Path, PathBuf};

use rustix::fs::{AtFlags, FileType, Mode, OFlags};
use sha2::{Digest as _, Sha256};

use super::{corrupt, io, permissions, same_file};
use crate::node::{Error, GraphBudget};
use crate::store::{self, bundle};

struct Directory {
    path: PathBuf,
    name: CString,
    parent: Option<usize>,
    file: File,
    metadata: Metadata,
    canonical: bool,
}

/// Private to trusted validation code. `observe` withholds its result until all
/// original attachments pass; the callback must not perform external execution.
pub(crate) struct Pending<'a> {
    graph: &'a GraphBudget,
    directories: Vec<Directory>,
    admitted: usize,
}

pub(crate) fn observe<T>(
    graph: &GraphBudget,
    operation: impl FnOnce(&mut Pending<'_>) -> Result<T, Error>,
) -> Result<T, Error> {
    let mut pending = Pending {
        graph,
        directories: Vec::new(),
        admitted: 0,
    };
    let result = operation(&mut pending);
    // Positive corruption can authorize retirement. A digest from a detached
    // directory must not identify a healthy replacement at the original path.
    if result.is_ok() || matches!(&result, Err(Error::CandidateCorrupt(_))) {
        pending.finish()?;
    }
    result
}

impl Pending<'_> {
    pub(crate) fn hash(
        &mut self,
        path: &Path,
        expected: &Metadata,
        limit: usize,
    ) -> Result<[u8; 32], Error> {
        let length = usize::try_from(expected.len()).map_err(|_| Error::Allocation)?;
        if length > limit {
            return Err(Error::Limit {
                resource: "runner artifact file bytes",
                limit,
                actual: length,
            });
        }
        // The digest is a pure function of the file bytes: one computation per
        // physical identity serves every later check in this process.
        let identity = crate::file_digest::Identity::of(expected);
        let memoized = crate::file_digest::get(identity, crate::file_digest::Domain::Sha256);
        if memoized.is_none() {
            self.graph
                .hash(length.checked_add(1).ok_or(Error::Allocation)?)?;
        }
        let parent = path.parent().ok_or_else(|| corrupt(path))?;
        let index = self.bind(parent)?;
        if !self.directories[index].canonical {
            self.canonical(index)?;
            self.directories[index].canonical = true;
        }
        let name = name(path.file_name().ok_or_else(|| corrupt(path))?, self.graph)?;
        let directory = &self.directories[index];
        self.graph.paths(1)?;
        let fd = rustix::fs::openat(
            &directory.file,
            name.as_c_str(),
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .map_err(|error| io("open retained relative file", path, error.into()))?;
        let mut file = File::from(fd);
        self.graph.paths(1)?;
        let opened = file
            .metadata()
            .map_err(|error| io("inspect retained relative file", path, error))?;
        if !opened.is_file()
            || bundle::hard_linked(&opened)
            || opened.len() != expected.len()
            || permissions(&opened) != permissions(expected)
            || !same_file(expected, &opened)
        {
            return Err(corrupt(path));
        }
        let opened_identity = crate::file_digest::Identity::of(&opened);
        if let Some(digest) = memoized {
            if opened_identity == identity {
                return Ok(digest);
            }
            self.graph
                .hash(length.checked_add(1).ok_or(Error::Allocation)?)?;
        }
        let mut digest = Sha256::new();
        let mut buffer = [0; 64 * 1024];
        let mut remaining = length;
        while remaining != 0 {
            let size = remaining.min(buffer.len());
            file.read_exact(&mut buffer[..size])
                .map_err(|error| io("hash retained relative file", path, error))?;
            digest.update(&buffer[..size]);
            remaining -= size;
        }
        if file
            .read(&mut buffer[..1])
            .map_err(|error| io("check retained relative file end", path, error))?
            != 0
        {
            return Err(corrupt(path));
        }
        self.graph.paths(1)?;
        let after = rustix::fs::statat(&directory.file, name.as_c_str(), AtFlags::SYMLINK_NOFOLLOW)
            .map_err(|error| io("reinspect retained relative file", path, error.into()))?;
        if !matches(&after, expected)
            || FileType::from_raw_mode(after.st_mode) != FileType::RegularFile
            || after.st_nlink != 1
            || u64::try_from(after.st_size).ok() != Some(expected.len())
        {
            return Err(corrupt(path));
        }
        let digest: [u8; 32] = digest.finalize().into();
        let reinspected = file
            .metadata()
            .ok()
            .and_then(|metadata| crate::file_digest::Identity::of(&metadata));
        if reinspected == opened_identity {
            crate::file_digest::insert(opened_identity, crate::file_digest::Domain::Sha256, digest);
        }
        Ok(digest)
    }

    fn bind(&mut self, path: &Path) -> Result<usize, Error> {
        if !path.is_absolute()
            || path
                .components()
                .any(|part| !matches!(part, Component::RootDir | Component::Normal(_)))
        {
            return Err(corrupt(path));
        }
        let mut parent = None;
        // Borrow prefixes without an unbounded recursive call stack or a second
        // allocated path list. Only previously absent directory slots are owned.
        for depth in (0..path.ancestors().count()).rev() {
            let prefix = path.ancestors().nth(depth).ok_or_else(|| corrupt(path))?;
            if let Some(index) = self
                .directories
                .iter()
                .position(|entry| entry.path == prefix)
            {
                parent = Some(index);
                continue;
            }
            let name = name(
                prefix.file_name().unwrap_or_else(|| OsStr::new("/")),
                self.graph,
            )?;
            let owned = bundle::path(prefix, Path::new(""), self.graph)?;
            self.reserve()?;
            self.graph.paths(1)?;
            let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
            let fd = match parent {
                Some(index) => rustix::fs::openat(
                    &self.directories[index].file,
                    name.as_c_str(),
                    flags,
                    Mode::empty(),
                ),
                None => rustix::fs::open(name.as_c_str(), flags, Mode::empty()),
            }
            .map_err(|error| io("open retained lookup directory", prefix, error.into()))?;
            let file = File::from(fd);
            self.graph.paths(1)?;
            let metadata = file
                .metadata()
                .map_err(|error| io("inspect retained lookup directory", prefix, error))?;
            if !metadata.is_dir() {
                return Err(corrupt(prefix));
            }
            let index = self.directories.len();
            self.directories.push(Directory {
                path: owned,
                name,
                parent,
                file,
                metadata,
                canonical: false,
            });
            parent = Some(index);
        }
        parent.ok_or_else(|| corrupt(path))
    }

    fn reserve(&mut self) -> Result<(), Error> {
        if self.directories.len() == self.admitted {
            let next = self
                .admitted
                .checked_mul(2)
                .ok_or(Error::Allocation)?
                .max(1);
            self.graph.decode(
                next.checked_mul(std::mem::size_of::<Directory>())
                    .ok_or(Error::Allocation)?,
            )?;
            self.directories
                .try_reserve_exact(next - self.directories.len())
                .map_err(|_| Error::Allocation)?;
            self.admitted = next;
        }
        Ok(())
    }

    fn canonical(&self, index: usize) -> Result<(), Error> {
        let path = &self.directories[index].path;
        self.graph.paths(1)?;
        store::path::canonical(path, self.graph)?;
        let current = std::fs::canonicalize(path)
            .map_err(|error| io("canonicalize retained lookup parent", path, error))?;
        if current != *path {
            return Err(corrupt(path));
        }
        Ok(())
    }

    fn finish(&self) -> Result<(), Error> {
        for (index, directory) in self.directories.iter().enumerate() {
            self.graph.paths(1)?;
            let (parent, name): (&File, &CStr) = match directory.parent {
                Some(parent) => (&self.directories[parent].file, directory.name.as_c_str()),
                None => (&directory.file, c"."),
            };
            let current =
                rustix::fs::statat(parent, name, AtFlags::SYMLINK_NOFOLLOW).map_err(|error| {
                    io(
                        "check retained directory attachment",
                        &directory.path,
                        error.into(),
                    )
                })?;
            if FileType::from_raw_mode(current.st_mode) != FileType::Directory
                || !matches(&current, &directory.metadata)
            {
                return Err(corrupt(&directory.path));
            }
            if directory.canonical {
                self.canonical(index)?;
            }
        }
        Ok(())
    }
}

fn name(value: &OsStr, graph: &GraphBudget) -> Result<CString, Error> {
    let length = value
        .as_bytes()
        .len()
        .checked_add(1)
        .ok_or(Error::Allocation)?;
    graph.decode(length)?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| Error::Allocation)?;
    bytes.extend_from_slice(value.as_bytes());
    bytes.push(0);
    CString::from_vec_with_nul(bytes).map_err(|_| Error::Codec("retained lookup name contains NUL"))
}

// dev_t and mode_t are i32 and u16 on macOS, u64 and u32 on Linux.
#[allow(clippy::unnecessary_cast, clippy::useless_conversion)]
fn matches(current: &rustix::fs::Stat, expected: &Metadata) -> bool {
    current.st_dev as u64 == expected.dev()
        && current.st_ino == expected.ino()
        && (u32::from(current.st_mode) & 0o7777) == permissions(expected)
}

#[cfg(test)]
mod tests;

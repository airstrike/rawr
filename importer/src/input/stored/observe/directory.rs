use std::ffi::{CStr, OsStr};
use std::os::unix::ffi::OsStrExt as _;
use std::path::Path;

use super::super::{Budget, Result};
use super::{io, package, path, read, slots, Error};
use crate::input::proof::{Entry, Role, TreeDirectory, TreeFile};

mod lookup;

struct Host<'borrow, 'account> {
    budget: &'borrow mut Budget<'account>,
}

impl rawr_directory::Admission for Host<'_, '_> {
    type Error = super::super::Error;

    fn admit(&mut self, work: rawr_directory::Work) -> Result<()> {
        use rawr_directory::{Storage, Work};
        self.budget.check()?;
        match work {
            Work::Open { .. } | Work::Reopen { .. } => self.budget.paths(2), // open and prospective one-shot cleanup
            Work::Mount | Work::Read { .. } | Work::Record | Work::EntryType { .. } => {
                self.budget.paths(1)
            }
            Work::Allocate { storage, bytes, .. } => {
                let items = match storage {
                    Storage::Entries => bytes / std::mem::size_of::<rawr_directory::Name>(),
                    Storage::Name => 1,
                };
                self.budget.storage(items, bytes)
            }
            Work::Visible { entries } => self.budget.limited(
                "input proof filesystem items",
                self.budget.limits.items,
                entries,
            ),
            Work::Retain { .. }
            | Work::Name { .. }
            | Work::Copy { .. }
            | Work::Order
            | Work::Compare { .. } => {
                // Same work bounds as store::bundle::directory: fixed
                // 8192-byte reads, >=24-byte records and admitted native names
                // bound framing, copying and allocation-free sorting. Raw
                // temporary records do not consume logical visible cardinality.
                Ok(())
            }
        }
    }
}

fn names(native: &CStr, budget: &mut Budget<'_>) -> Result<rawr_directory::Names> {
    let mut buffer = [0; rawr_directory::BUFFER_BYTES];
    let mut host = Host { budget };
    rawr_directory::collect(native, &mut buffer, &mut host)
        .and_then(|names| names.sorted(&mut host))
        .map_err(|error| match error {
            rawr_directory::Error::Admission(error) => error,
            rawr_directory::Error::Io { operation, source } => io(operation, source),
            rawr_directory::Error::Framing(_) => {
                Error::Policy("native directory framing is malformed").into()
            }
            rawr_directory::Error::Terminal => {
                Error::Policy("native directory reader is terminal").into()
            }
            rawr_directory::Error::Allocation => host.budget.failure(Error::Allocation.into()),
        })
}

fn metadata(
    lookup: &lookup::Lookup,
    name: &Path,
    kind: u8,
    budget: &mut Budget<'_>,
) -> Result<lookup::Metadata> {
    let mut native = None;
    inspect(kind, budget, |budget| {
        if native.is_none() {
            native = Some(path::terminated(name, budget)?);
        }
        let native = native
            .as_deref()
            .ok_or(Error::Policy("input member spelling is absent"))?;
        let native = CStr::from_bytes_with_nul(native)
            .map_err(|_| Error::Policy("input member contains NUL"))?;
        lookup.metadata(native, budget)
    })
}

fn inspect(
    kind: u8,
    budget: &mut Budget<'_>,
    mut metadata: impl FnMut(&mut Budget<'_>) -> Result<lookup::Metadata>,
) -> Result<lookup::Metadata> {
    budget.check()?;
    // Pinned std/sys/fs/unix.rs:1140–1151 uses these seven cached d_type
    // encodings and metadata fallback for every other value. Its subsequent
    // DirEntry::metadata uses AT_SYMLINK_NOFOLLOW (1074–1102).
    match kind {
        libc::DT_LNK => return Err(Error::Policy("symlink-input").into()),
        libc::DT_CHR
        | libc::DT_FIFO
        | libc::DT_REG
        | libc::DT_SOCK
        | libc::DT_DIR
        | libc::DT_BLK => {}
        _ => {
            if metadata(budget)?.is_symlink() {
                return Err(Error::Policy("symlink-input").into());
            }
        }
    }
    metadata(budget)
}

pub(super) fn entry(
    role: Role,
    package_id: Option<&str>,
    source: &Path,
    base: &Path,
    budget: &mut Budget<'_>,
) -> Result<Entry> {
    walk(role, package_id, source, base, budget, |_| Ok(()))
}

fn walk(
    role: Role,
    package_id: Option<&str>,
    source: &Path,
    base: &Path,
    budget: &mut Budget<'_>,
    mut collected: impl FnMut(&Path) -> Result<()>,
) -> Result<Entry> {
    let root = path::canonical(source, base, true, budget)?;
    let mut pending = slots::Slots::new();
    pending.push(
        (
            budget.owned_path(root.as_os_str().as_encoded_bytes())?,
            0_usize,
        ),
        budget,
    )?;
    let mut directories = slots::Slots::new();
    let mut files = slots::Slots::new();
    while let Some((directory, depth)) = pending.pop() {
        budget.physical("input directory depth", budget.limits.depth, depth)?;
        let native = path::terminated(&directory, budget)?;
        let native = CStr::from_bytes_with_nul(&native)
            .map_err(|_| Error::Policy("input path contains NUL"))?;
        let lookup = lookup::Lookup::open(native, budget)?;
        lookup.revalidate(native, budget)?;
        let names = names(native, budget)?;
        lookup.revalidate(native, budget)?;
        collected(&directory)?;
        for name in names.into_iter().rev() {
            let native_name = Path::new(OsStr::from_bytes(name.bytes()));
            let child = path::joined(&directory, native_name, budget)?;
            let metadata = metadata(&lookup, native_name, name.kind(), budget)?;
            let relative = child
                .strip_prefix(&root)
                .map_err(|_| Error::Policy("directory-escape"))?;
            let relative = path::text(relative, budget)?;
            if metadata.is_dir() {
                let depth = depth.checked_add(1).ok_or_else(|| budget.overflow())?;
                budget.physical("input directory depth", budget.limits.depth, depth)?;
                pending.push((child, depth), budget)?;
                directories.push(TreeDirectory { path: relative }, budget)?;
            } else if metadata.is_file() {
                let bytes = read::length(metadata.len(), budget)?;
                let digest = read::digest(&child, bytes, budget)?;
                files.push(
                    TreeFile {
                        path: relative,
                        bytes,
                        digest,
                    },
                    budget,
                )?;
            } else {
                return Err(Error::Policy("unsupported-input-shape").into());
            }
        }
        lookup.revalidate(native, budget)?;
    }
    Ok(Entry::Directory {
        role,
        package_id: package(package_id, budget)?,
        path: path::text(&root, budget)?,
        directories: directories.sorted(budget)?,
        files: files.sorted(budget)?,
    })
}

#[cfg(test)]
mod tests;

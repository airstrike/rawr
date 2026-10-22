//! Versioned filesystem identities for a supervised runner's watcher.
//!
//! This carries observation coverage only. Decoding neither reads file contents
//! nor establishes current positive/negative membership or admission authority.

use std::ffi::{OsStr, OsString};
use std::fs;
use std::mem::size_of;
use std::os::unix::ffi::{OsStrExt as _, OsStringExt as _};
use std::path::{Path, PathBuf};

use super::Inputs;

/// Maximum payload after the wire prefix and authenticated generation key.
pub const MAX_BYTES: usize = rawr_import::protocol::FRAME_BYTES - 5 - 32;
const VERSION: u8 = 2;
const HEADER: usize = 13;

/// An observation cannot be transported within the format's bounds.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum Error {
    /// Unsupported schema or malformed, noncanonical, unordered data.
    #[error("malformed filesystem observation transport")]
    Malformed,
    /// Wire bytes or retained path/layout storage exceed the bounded budget.
    #[error("filesystem observation transport exceeds its bounded budget")]
    Limit,
    /// The allocator refused bounded transport storage.
    #[error("cannot allocate filesystem observation transport")]
    Allocation,
}

impl Inputs {
    /// Encode exact paths and recursive roots without lossy path conversion.
    ///
    /// Derived watcher directories are excluded. Both wire storage and a
    /// conservative decoded path/layout bound must fit the transport budget.
    pub fn encode_observation(&self) -> Result<Box<[u8]>, Error> {
        let mut wire = HEADER;
        let mut retained = 0;
        for (paths, exact) in [
            (&*self.paths, true),
            (&*self.trees, false),
            (&*self.sources, false),
        ] {
            u32::try_from(paths.len()).map_err(|_| Error::Limit)?;
            let mut previous = None;
            for path in paths {
                validate(path, previous)?;
                previous = Some(path.as_path());
                let bytes = path.as_os_str().as_bytes();
                charge(&mut wire, 4)?;
                charge(&mut wire, bytes.len())?;
                charge_path(&mut retained, bytes.len(), exact)?;
            }
        }
        if self
            .sources
            .iter()
            .any(|path| self.paths.binary_search(path).is_err())
            || self.trees.iter().any(|tree| {
                self.sources
                    .get(self.sources.partition_point(|source| source < tree))
                    .is_some_and(|source| source.starts_with(tree))
            })
        {
            return Err(Error::Malformed);
        }
        let mut output = reserve(wire)?;
        output.push(VERSION);
        output.extend_from_slice(&(self.paths.len() as u32).to_le_bytes());
        output.extend_from_slice(&(self.trees.len() as u32).to_le_bytes());
        output.extend_from_slice(&(self.sources.len() as u32).to_le_bytes());
        for path in self
            .paths
            .iter()
            .chain(self.trees.iter())
            .chain(self.sources.iter())
        {
            let bytes = path.as_os_str().as_bytes();
            output.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
            output.extend_from_slice(bytes);
        }
        Ok(output.into_boxed_slice())
    }
}

/// Validated filesystem observation identities, without freshness authority.
#[derive(Debug)]
pub struct Decoded {
    paths: Box<[PathBuf]>,
    trees: Box<[PathBuf]>,
    sources: Box<[PathBuf]>,
}

/// Decode observation identities without filesystem I/O or allocation until
/// the complete payload and retained path/layout budget have been checked.
pub fn decode(bytes: &[u8]) -> Result<Decoded, Error> {
    let (exact, trees, sources) = inspect(bytes)?;
    let mut reader = Reader(&bytes[HEADER..]);
    let mut paths = reserve(exact)?;
    for _ in 0..exact {
        paths.push(own_path(reader.path()?)?);
    }
    let mut roots = reserve(trees)?;
    for _ in 0..trees {
        roots.push(own_path(reader.path()?)?);
    }
    let mut source_paths = reserve(sources)?;
    for _ in 0..sources {
        source_paths.push(own_path(reader.path()?)?);
    }
    Ok(Decoded {
        paths: paths.into_boxed_slice(),
        trees: roots.into_boxed_slice(),
        sources: source_paths.into_boxed_slice(),
    })
}

impl Decoded {
    /// Whether these are exactly `inputs`' identities, whose derived
    /// directories then need no second filesystem pass: any change to them
    /// since is a watched event that revokes the request carrying this.
    #[must_use]
    pub fn identifies(&self, inputs: &Inputs) -> bool {
        self.paths == inputs.paths && self.trees == inputs.trees && self.sources == inputs.sources
    }

    /// Reconstruct derived watcher directories using the current filesystem.
    ///
    /// Missing files and parents remain valid identities. Retained directory
    /// paths/layout fit the decoder's reserved budget; temporary filesystem
    /// canonicalization storage is outside that retained-storage contract.
    #[must_use]
    pub fn into_inputs(self) -> Inputs {
        let Self {
            paths,
            trees,
            sources,
        } = self;
        // Inspection charged the full exact path length and PathBuf layout a
        // second time for every possible ancestor. Each retained ancestor is a
        // lexical prefix; canonicalization only confirms that identity.
        // Sibling files share one parent; canonicalize each parent once.
        let mut parents = paths
            .iter()
            .filter_map(|path| path.parent())
            .collect::<Vec<_>>();
        parents.sort_unstable();
        parents.dedup();
        let mut directories = Vec::with_capacity(parents.len());
        for parent in parents {
            if let Some(directory) = parent.ancestors().find_map(|candidate| {
                fs::canonicalize(candidate)
                    .ok()
                    .filter(|canonical| canonical == candidate && canonical.is_dir())
                    .map(|_| candidate.to_path_buf())
            }) {
                directories.push(directory);
            }
        }
        directories.sort_unstable();
        directories.dedup();
        Inputs {
            paths,
            directories: directories.into_boxed_slice(),
            trees,
            sources,
        }
    }
}

fn inspect(bytes: &[u8]) -> Result<(usize, usize, usize), Error> {
    if bytes.len() > MAX_BYTES {
        return Err(Error::Limit);
    }
    if bytes.len() < HEADER || bytes[0] != VERSION {
        return Err(Error::Malformed);
    }
    let mut reader = Reader(&bytes[1..]);
    let exact = reader.number()?;
    let trees = reader.number()?;
    let sources = reader.number()?;
    // Reject malicious counts before iteration, and before any allocation.
    let count = exact
        .checked_add(trees)
        .and_then(|count| count.checked_add(sources))
        .ok_or(Error::Limit)?;
    if count > reader.0.len() / 5 {
        return Err(Error::Malformed);
    }
    let mut retained = 0;
    let mut exact_paths = reader;
    inspect_paths(&mut reader, exact, &mut retained, true)?;
    let mut roots = reader;
    inspect_paths(&mut reader, trees, &mut retained, false)?;
    let source_paths = reader;
    inspect_paths(&mut reader, sources, &mut retained, false)?;
    if !reader.0.is_empty() {
        return Err(Error::Malformed);
    }
    // Merge ordered tables without allocating or repeatedly rescanning them.
    let mut source_reader = source_paths;
    let mut remaining = exact;
    for _ in 0..sources {
        let source = source_reader.path()?;
        loop {
            if remaining == 0 {
                return Err(Error::Malformed);
            }
            remaining -= 1;
            match exact_paths.path()?.cmp(source) {
                std::cmp::Ordering::Less => continue,
                std::cmp::Ordering::Equal => break,
                std::cmp::Ordering::Greater => return Err(Error::Malformed),
            }
        }
    }
    let mut source_reader = source_paths;
    let mut remaining = sources;
    let mut source = None;
    for _ in 0..trees {
        let tree = roots.path()?;
        while source.is_none_or(|path| path < tree) && remaining > 0 {
            source = Some(source_reader.path()?);
            remaining -= 1;
        }
        if source.is_some_and(|path| path.starts_with(tree)) {
            return Err(Error::Malformed);
        }
    }
    Ok((exact, trees, sources))
}

fn inspect_paths(
    reader: &mut Reader<'_>,
    count: usize,
    retained: &mut usize,
    exact: bool,
) -> Result<(), Error> {
    let mut previous = None;
    for _ in 0..count {
        let path = reader.path()?;
        validate(path, previous)?;
        previous = Some(path);
        charge_path(retained, path.as_os_str().as_bytes().len(), exact)?;
    }
    Ok(())
}

fn validate(path: &Path, previous: Option<&Path>) -> Result<(), Error> {
    let bytes = path.as_os_str().as_bytes();
    if bytes.first() != Some(&b'/') || bytes.contains(&0) {
        return Err(Error::Malformed);
    }
    if bytes.len() > 1
        && bytes[1..]
            .split(|byte| *byte == b'/')
            .any(|part| part.is_empty() || part == b"." || part == b"..")
    {
        return Err(Error::Malformed);
    }
    if previous.is_some_and(|previous| previous >= path) {
        return Err(Error::Malformed);
    }
    Ok(())
}

fn charge(total: &mut usize, amount: usize) -> Result<(), Error> {
    *total = total.checked_add(amount).ok_or(Error::Limit)?;
    if *total > MAX_BYTES {
        return Err(Error::Limit);
    }
    Ok(())
}

fn charge_path(total: &mut usize, length: usize, exact: bool) -> Result<(), Error> {
    let amount = length
        .checked_add(size_of::<PathBuf>())
        .ok_or(Error::Limit)?;
    charge(total, amount)?;
    if exact {
        charge(total, amount)?;
    }
    Ok(())
}

fn reserve<T>(capacity: usize) -> Result<Vec<T>, Error> {
    let mut values = Vec::new();
    values
        .try_reserve_exact(capacity)
        .map_err(|_| Error::Allocation)?;
    Ok(values)
}

fn own_path(path: &Path) -> Result<PathBuf, Error> {
    let bytes = path.as_os_str().as_bytes();
    let mut owned = reserve(bytes.len())?;
    owned.extend_from_slice(bytes);
    Ok(OsString::from_vec(owned).into())
}

#[derive(Clone, Copy)]
struct Reader<'a>(&'a [u8]);

impl<'a> Reader<'a> {
    fn number(&mut self) -> Result<usize, Error> {
        let bytes = self.0.get(..4).ok_or(Error::Malformed)?;
        let value = u32::from_le_bytes(bytes.try_into().map_err(|_| Error::Malformed)?);
        self.0 = &self.0[4..];
        usize::try_from(value).map_err(|_| Error::Limit)
    }

    fn path(&mut self) -> Result<&'a Path, Error> {
        let length = self.number()?;
        let bytes = self.0.get(..length).ok_or(Error::Malformed)?;
        self.0 = &self.0[length..];
        Ok(Path::new(OsStr::from_bytes(bytes)))
    }
}

#[cfg(test)]
mod tests;

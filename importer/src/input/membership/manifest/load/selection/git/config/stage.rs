// Copyright the gitoxide contributors. MIT OR Apache-2.0.
//! gix 0.85.0 local/worktree StageOne, default lossy=false and lenient=true.

use super::super::path;
use super::{
    parse, storage, trust, value, Admission, Error, File, Metadata, Session, Source, Step,
};
use crate::input::membership::{
    self, git_read, host,
    manifest::{io, load},
};
use std::{
    io::ErrorKind,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub(in super::super) struct Stage {
    config: File,
    buffer: git_read::Buffer,
    is_bare: Option<bool>,
    lossy: bool,
    object_hash: Hash,
    reflog: Option<Reflog>,
    precompose_unicode: bool,
    protect_windows: bool,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Hash {
    Sha1,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Reflog {
    Normal,
    Disable,
    Always,
}

#[derive(Debug)]
pub(in super::super) enum Failure {
    Parse(parse::Failure),
    Unsigned(Key),
    Typed(Key),
    ObjectFormatRequiresV1,
    UnsupportedRepositoryFormatVersion { version: usize },
    Io { source: io::Failure, path: PathBuf },
}
#[derive(Debug)]
pub(super) struct Key {
    key: Vec<u8>,
    value: Option<Vec<u8>>,
    environment_override: Option<&'static str>,
    source: Option<value::Failure>,
}

impl Stage {
    pub(in super::super) fn precompose_unicode(&self) -> bool {
        self.precompose_unicode
    }

    pub(in super::super) fn local(
        common: &Path,
        git_dir: &Path,
        trust: trust::Level,
        session: &mut Session<'_, host::Host<'_, '_>>,
    ) -> Step<Result<Self, Failure>> {
        let mut buffer = session.git_buffer()?;
        let config_path = path::file::join(common, Path::new("config"), session)?;
        let mut config = match load(config_path, &mut buffer, Source::Local, trust, session)? {
            Ok(config) => config,
            Err(error) => return Ok(Err(error)),
        };
        let is_bare = boolean(&config, b"core", None, b"bare", session)?;
        session.items(b"core.repositoryFormatVersion".len())?;
        let version = match config.integer(b"core", b"repositoryFormatVersion", session)? {
            None => 0,
            Some(Ok(value)) => match usize::try_from(value) {
                Ok(value) => value,
                Err(_) => {
                    return Ok(Err(Failure::Unsigned(key(
                        b"core",
                        None,
                        b"repositoryFormatVersion",
                        None,
                        None,
                        session,
                    )?)))
                }
            },
            Some(Err(error)) => {
                return Ok(Err(Failure::Unsigned(key(
                    b"core",
                    None,
                    b"repositoryFormatVersion",
                    None,
                    Some(error),
                    session,
                )?)))
            }
        };
        let format = config.string(b"extensions", None, b"objectFormat", session)?;
        let object_hash = match (version, format) {
            (1, Some(format)) => {
                session.items(format.len().max(4))?;
                if !format.eq_ignore_ascii_case(b"sha1") {
                    let value = value::into_owned(format, session)?;
                    return Ok(Err(Failure::Typed(key(
                        b"extensions",
                        None,
                        b"objectFormat",
                        Some(value),
                        None,
                        session,
                    )?)));
                }
                Hash::Sha1
            }
            (0, Some(_)) => return Ok(Err(Failure::ObjectFormatRequiresV1)),
            (0 | 1, None) => Hash::Sha1,
            (version, _) => {
                return Ok(Err(Failure::UnsupportedRepositoryFormatVersion { version }))
            }
        };
        if boolean(&config, b"extensions", None, b"worktreeConfig", session)?.unwrap_or(false) {
            let file = path::file::join(git_dir, Path::new("config.worktree"), session)?;
            let worktree = match load(file, &mut buffer, Source::Worktree, trust, session)? {
                Ok(config) => config,
                Err(error) => return Ok(Err(error)),
            };
            config.append(worktree, session)?;
        }
        let precompose_unicode =
            boolean(&config, b"core", None, b"precomposeUnicode", session)?.unwrap_or(false);
        let protect_windows = boolean(
            &config,
            b"gitoxide",
            Some(b"core"),
            b"protectWindows",
            session,
        )?
        .unwrap_or(false);
        session.items(b"core.logAllRefUpdates".len())?;
        let reflog = match config.boolean(b"core", None, b"logAllRefUpdates", session)? {
            None => None,
            Some(Ok(value)) => Some(if value {
                Reflog::Normal
            } else {
                Reflog::Disable
            }),
            Some(Err(error)) => {
                let value = error.input;
                session.items(value.len().max(6))?;
                if value.eq_ignore_ascii_case(b"always") {
                    Some(Reflog::Always)
                } else {
                    // Build the exact reached typed error before leniency drops it.
                    drop(key(
                        b"core",
                        None,
                        b"logAllRefUpdates",
                        Some(value),
                        None,
                        session,
                    )?);
                    None
                }
            }
        };
        Ok(Ok(Self {
            config,
            buffer,
            is_bare,
            lossy: false,
            object_hash,
            reflog,
            precompose_unicode,
            protect_windows,
        }))
    }
}

fn key<A: Admission>(
    section: &[u8],
    sub: Option<&[u8]>,
    name: &[u8],
    value: Option<Vec<u8>>,
    source: Option<value::Failure>,
    session: &mut Session<'_, A>,
) -> Step<Key> {
    // Key::logical_name builds a String with separate push_str/push calls;
    // BString then takes that byte allocation without copying or shrinking.
    let mut bytes = storage::Vector::new();
    bytes.extend_copy(section, session)?;
    bytes.push(b'.', session)?;
    if let Some(sub) = sub {
        bytes.extend_copy(sub, session)?;
        bytes.push(b'.', session)?;
    }
    bytes.extend_copy(name, session)?;
    Ok(Key {
        key: bytes.into_values(),
        value,
        environment_override: None,
        source,
    })
}

fn boolean<A: Admission>(
    config: &File,
    section: &'static [u8],
    sub: Option<&'static [u8]>,
    name: &[u8],
    session: &mut Session<'_, A>,
) -> Step<Option<bool>> {
    match config.boolean(section, sub, name, session)? {
        None => Ok(None),
        Some(Ok(value)) => Ok(Some(value)),
        Some(Err(error)) => {
            drop(key(section, sub, name, None, Some(error), session)?);
            Ok(Some(false))
        }
    }
}

fn load(
    path: PathBuf,
    buffer: &mut git_read::Buffer,
    source: Source,
    trust: trust::Level,
    session: &mut Session<'_, host::Host<'_, '_>>,
) -> Step<Result<File, Failure>> {
    session.items(path.as_os_str().len())?;
    let metadata = Metadata {
        path: Some(load::path::copy(&path, session)?),
        source,
        level: 0,
        trust,
    };
    let bytes = path.as_os_str().as_bytes();
    session.string(bytes.len())?;
    session.items(bytes.len())?;
    if bytes.contains(&0) {
        // Preserve CString's allocation-before-NUL-rejection boundary locally.
        // The native observation Path must never receive this invalid spelling.
        if bytes.len() < 384 {
            let mut native = [0u8; 384];
            session.items(bytes.len())?;
            native[..bytes.len()].copy_from_slice(bytes);
            if std::ffi::CStr::from_bytes_with_nul(&native[..bytes.len() + 1]).is_ok() {
                return session.fail(Error::State);
            }
        } else {
            let Some(capacity) = bytes.len().checked_add(1) else {
                return session.fail(Error::Overflow);
            };
            let mut native = storage::Vector::with_capacity(capacity, session)?;
            native.extend_copy(bytes, session)?;
            session.items(bytes.len())?;
            if !native.as_slice().contains(&0) {
                return session.fail(Error::State);
            }
        }
        let error = Failure::Io {
            source: io::Failure::Nul,
            path,
        };
        let result = File::empty(metadata, session)?;
        drop(error);
        return Ok(Ok(result));
    }
    session.git_configuration(path, buffer, move |outcome, path, buffer, budget| {
        let mut local = Session::new(budget);
        let result = complete(outcome, path, buffer, metadata, &mut local);
        local.finish(result).map_err(bridge)
    })
}

fn complete<A: Admission>(
    outcome: git_read::Configuration,
    path: &mut Option<PathBuf>,
    buffer: &mut git_read::Buffer,
    metadata: Metadata,
    session: &mut Session<'_, A>,
) -> Step<Result<File, Failure>> {
    match outcome {
        git_read::Configuration::OpenFailed(code)
            if io::Failure::Native(code).kind() == ErrorKind::NotFound =>
        {
            return File::empty(metadata, session).map(Ok)
        }
        git_read::Configuration::OpenFailed(code) => {
            let Some(path) = path.take() else {
                return session.fail(Error::State);
            };
            let error = Failure::Io {
                source: io::Failure::Native(code),
                path,
            };
            let result = File::empty(metadata, session)?;
            drop(error);
            return Ok(Ok(result));
        }
        git_read::Configuration::Read(Err(code)) => {
            let Some(path) = path.take() else {
                return session.fail(Error::State);
            };
            let error = Failure::Io {
                source: io::Failure::Native(code),
                path,
            };
            buffer.clear();
            drop(error);
        }
        git_read::Configuration::Read(Ok(())) => {}
    }
    File::parse(buffer.bytes(), metadata, session).map(|result| result.map_err(Failure::Parse))
}

fn bridge(error: Error<membership::Error>) -> membership::Error {
    match error {
        Error::Admission(error) | Error::Host(error) => error,
        Error::Allocation(error) => membership::Error::Allocation(error),
        Error::InlineAllocation(error) => membership::Error::InlineAllocation(error),
        Error::Overflow => membership::Error::DocumentOverflow,
        _ => membership::Error::Encoding("Git configuration completion failed"),
    }
}

#[cfg(test)]
mod tests;

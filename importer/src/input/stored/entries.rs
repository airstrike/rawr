//! Exact expected-proof data and its existing canonical fingerprint.

use super::codec::{Reader, Writer};
use super::{Budget, Error, Result};
use crate::input::{fingerprint, proof, replay};

impl Writer<'_> {
    pub(super) fn role(&mut self, role: proof::Role) -> Result<()> {
        self.byte(match role {
            proof::Role::Authority => 0,
            proof::Role::Harness => 1,
            proof::Role::Config => 2,
            proof::Role::Toolchain => 3,
            proof::Role::Target => 4,
            proof::Role::Profile => 5,
            proof::Role::PackageManifest => 6,
            proof::Role::DependencySource => 7,
            proof::Role::DepInfo => 8,
            proof::Role::BuildScript => 9,
            proof::Role::DeclaredInput => 10,
            proof::Role::DefaultPackageSource => 11,
            proof::Role::OutDirectory => 12,
            proof::Role::BuiltOutput => 13,
            proof::Role::BuildMessage => 14,
        })
    }

    pub(super) fn entries(&mut self, entries: &[proof::Entry]) -> Result<()> {
        self.count(entries.len())?;
        for entry in entries {
            self.entry(entry)?;
        }
        Ok(())
    }

    fn files(&mut self, files: &[proof::TreeFile]) -> Result<()> {
        self.count(files.len())?;
        for file in files {
            self.text(&file.path)?;
            self.count(file.bytes)?;
            self.raw(&file.digest)?;
        }
        Ok(())
    }

    fn entry(&mut self, entry: &proof::Entry) -> Result<()> {
        match entry {
            proof::Entry::Workspace { paths, digest } => {
                self.byte(10)?;
                self.count(paths.len())?;
                for path in paths {
                    self.text(path)?;
                }
                self.raw(digest)
            }
            proof::Entry::File {
                role,
                package_id,
                path,
                bytes,
                digest,
            } => {
                self.byte(0)?;
                self.role(*role)?;
                self.option(package_id.as_deref())?;
                self.text(path)?;
                self.count(*bytes)?;
                self.raw(digest)
            }
            proof::Entry::Directory {
                role,
                package_id,
                path,
                directories,
                files,
            } => {
                self.byte(1)?;
                self.role(*role)?;
                self.option(package_id.as_deref())?;
                self.text(path)?;
                self.count(directories.len())?;
                for directory in directories {
                    self.text(&directory.path)?;
                }
                self.files(files)
            }
            proof::Entry::Registry {
                role,
                package_id,
                source,
                checksum,
                root,
                marker_schema,
                marker_digest,
            } => {
                self.byte(2)?;
                self.role(*role)?;
                self.text(package_id)?;
                self.text(source)?;
                self.raw(checksum)?;
                self.text(root)?;
                self.u32(*marker_schema)?;
                self.raw(marker_digest)
            }
            proof::Entry::Git {
                role,
                package_id,
                source,
                oid,
                root,
                files,
            } => {
                self.byte(3)?;
                self.role(*role)?;
                self.text(package_id)?;
                self.text(source)?;
                self.text(oid)?;
                self.text(root)?;
                self.files(files)
            }
            proof::Entry::Missing {
                role,
                package_id,
                path,
            } => {
                self.byte(4)?;
                self.role(*role)?;
                self.option(package_id.as_deref())?;
                self.text(path)
            }
            proof::Entry::Environment {
                package_id,
                execution,
                key,
                value,
            } => {
                self.byte(5)?;
                self.text(package_id)?;
                self.text(execution)?;
                self.text(key)?;
                self.option(value.as_deref())
            }
            proof::Entry::EnvironmentProjection { schema, key, value } => {
                self.byte(6)?;
                self.u32(*schema)?;
                self.text(key)?;
                self.option(value.as_deref())
            }
            proof::Entry::Fact {
                role,
                package_id,
                name,
                value,
            } => {
                self.byte(7)?;
                self.role(*role)?;
                self.option(package_id.as_deref())?;
                self.text(name)?;
                self.text(value)
            }
            proof::Entry::BuildMessage {
                package_id,
                execution,
                name,
                source,
                value,
            } => {
                self.byte(8)?;
                self.text(package_id)?;
                self.text(execution)?;
                self.text(name)?;
                self.option(source.as_deref())?;
                self.text(value)
            }
            proof::Entry::Directive {
                package_id,
                execution,
                index,
                value,
            } => {
                self.byte(9)?;
                self.text(package_id)?;
                self.text(execution)?;
                self.count(*index)?;
                self.text(value)
            }
        }
    }
}

impl Reader<'_, '_, '_> {
    pub(super) fn role(&mut self) -> Result<proof::Role> {
        Ok(match self.byte()? {
            0 => proof::Role::Authority,
            1 => proof::Role::Harness,
            2 => proof::Role::Config,
            3 => proof::Role::Toolchain,
            4 => proof::Role::Target,
            5 => proof::Role::Profile,
            6 => proof::Role::PackageManifest,
            7 => proof::Role::DependencySource,
            8 => proof::Role::DepInfo,
            9 => proof::Role::BuildScript,
            10 => proof::Role::DeclaredInput,
            11 => proof::Role::DefaultPackageSource,
            12 => proof::Role::OutDirectory,
            13 => proof::Role::BuiltOutput,
            14 => proof::Role::BuildMessage,
            _ => return Err(Error::Encoding("unknown proof role")),
        })
    }

    pub(super) fn entries(&mut self) -> Result<Box<[proof::Entry]>> {
        let count = self.count()?;
        if count > self.remaining() / 3 {
            return Err(Error::Encoding("impossible entry count"));
        }
        let mut entries: Vec<proof::Entry> = self.budget.items(count)?;
        for _ in 0..count {
            let entry = self.entry()?;
            if entries.last().is_some_and(|previous| previous >= &entry) {
                return Err(Error::Encoding("expected entries are not strictly ordered"));
            }
            entries.push(entry);
        }
        self.budget.boxed(entries)
    }

    fn files(&mut self) -> Result<Box<[proof::TreeFile]>> {
        let count = self.count()?;
        if count > self.remaining() / 48 {
            return Err(Error::Encoding("impossible tree file count"));
        }
        let mut files: Vec<proof::TreeFile> = self.budget.items(count)?;
        for _ in 0..count {
            let file = proof::TreeFile {
                path: self.text_path()?,
                bytes: self.length()?,
                digest: self.digest()?,
            };
            if files.last().is_some_and(|previous| previous >= &file) {
                return Err(Error::Encoding("tree files are not strictly ordered"));
            }
            files.push(file);
        }
        self.budget.boxed(files)
    }

    fn directories(&mut self) -> Result<Box<[proof::TreeDirectory]>> {
        let count = self.count()?;
        if count > self.remaining() / 8 {
            return Err(Error::Encoding("impossible tree directory count"));
        }
        let mut directories: Vec<proof::TreeDirectory> = self.budget.items(count)?;
        for _ in 0..count {
            let directory = proof::TreeDirectory {
                path: self.text_path()?,
            };
            if directories
                .last()
                .is_some_and(|previous| previous >= &directory)
            {
                return Err(Error::Encoding("tree directories are not strictly ordered"));
            }
            directories.push(directory);
        }
        self.budget.boxed(directories)
    }

    fn entry(&mut self) -> Result<proof::Entry> {
        Ok(match self.byte()? {
            10 => {
                let count = self.count()?;
                if count > self.remaining() / 8 {
                    return Err(Error::Encoding("impossible workspace watch count"));
                }
                let mut paths = self.budget.items(count)?;
                for _ in 0..count {
                    let path = self.text_path()?;
                    if paths.last().is_some_and(|previous| previous >= &path) {
                        return Err(Error::Encoding(
                            "workspace watch paths are not strictly ordered",
                        ));
                    }
                    paths.push(path);
                }
                proof::Entry::Workspace {
                    paths: self.budget.boxed(paths)?,
                    digest: self.digest()?,
                }
            }
            0 => proof::Entry::File {
                role: self.role()?,
                package_id: self.option()?,
                path: self.text_path()?,
                bytes: self.length()?,
                digest: self.digest()?,
            },
            1 => proof::Entry::Directory {
                role: self.role()?,
                package_id: self.option()?,
                path: self.text_path()?,
                directories: self.directories()?,
                files: self.files()?,
            },
            2 => proof::Entry::Registry {
                role: self.role()?,
                package_id: self.text()?,
                source: self.text()?,
                checksum: self.digest()?,
                root: self.text_path()?,
                marker_schema: self.u32()?,
                marker_digest: self.digest()?,
            },
            3 => proof::Entry::Git {
                role: self.role()?,
                package_id: self.text()?,
                source: self.text()?,
                oid: self.text()?,
                root: self.text_path()?,
                files: self.files()?,
            },
            4 => proof::Entry::Missing {
                role: self.role()?,
                package_id: self.option()?,
                path: self.text_path()?,
            },
            5 => proof::Entry::Environment {
                package_id: self.text()?,
                execution: self.text()?,
                key: self.text()?,
                value: self.option()?,
            },
            6 => proof::Entry::EnvironmentProjection {
                schema: self.u32()?,
                key: self.text()?,
                value: self.option()?,
            },
            7 => {
                let role = self.role()?;
                let package_id = self.option()?;
                // Dep-info facts synthesize their path-bearing name and hex
                // digest under the aggregate account, not the scalar limit.
                let scalar = role != proof::Role::DepInfo;
                let name = self.bytes()?;
                let name = self.budget.text(name, scalar)?;
                let value = self.bytes()?;
                let value = self.budget.text(value, scalar)?;
                proof::Entry::Fact {
                    role,
                    package_id,
                    name,
                    value,
                }
            }
            8 => {
                let package_id = self.text()?;
                let execution = self.bytes()?;
                let name = self.text()?;
                let source = self.option_path()?;
                let scalar = source.is_none();
                let execution = self.budget.text(execution, scalar)?;
                let value = self.bytes()?;
                let value = self.budget.text(value, scalar)?;
                proof::Entry::BuildMessage {
                    package_id,
                    execution,
                    name,
                    source,
                    value,
                }
            }
            9 => proof::Entry::Directive {
                package_id: self.text()?,
                execution: self.text()?,
                index: self.length()?,
                value: self.text()?,
            },
            _ => return Err(Error::Encoding("unknown expected entry tag")),
        })
    }
}

pub(super) fn fingerprint(
    schema: u32,
    entries: &[proof::Entry],
    budget: &mut Budget<'_>,
) -> Result<fingerprint::Fingerprint> {
    let digest = replay::canonical::digest(
        &(schema, entries),
        "canonical expected proof bytes",
        budget.limits.input_proof_bytes,
        budget,
    )?;
    Ok(fingerprint::Fingerprint::from_bytes(digest))
}

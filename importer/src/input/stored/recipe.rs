//! Replay descriptions containing only retained data.

use std::path::PathBuf;

use super::codec::{Reader, Writer};
use super::view::View;
use super::{Error, Result};
use crate::input::{package, proof};

pub(crate) enum Recipe {
    Workspace {
        roots: Box<[PathBuf]>,
    },
    File {
        role: proof::Role,
        package_id: Option<Box<str>>,
        path: PathBuf,
    },
    Path {
        role: proof::Role,
        package_id: Option<Box<str>>,
        path: PathBuf,
    },
    Directory {
        role: proof::Role,
        package_id: Option<Box<str>>,
        path: PathBuf,
    },
    DepInfo {
        package_id: Box<str>,
        path: PathBuf,
        package_root: PathBuf,
        out_directories: Box<[PathBuf]>,
    },
    SavedOutput {
        package_id: Box<str>,
        out_dir: PathBuf,
    },
    Package(Package),
    Environment {
        package_id: Box<str>,
        execution: Box<str>,
        key: Box<str>,
        value: Option<Box<str>>,
    },
    EnvironmentProjection {
        schema: u32,
        key: Box<str>,
        value: Option<Box<str>>,
    },
    Fact {
        role: proof::Role,
        package_id: Option<Box<str>>,
        name: Box<str>,
        value: Box<str>,
    },
    BuildMessage {
        package_id: Box<str>,
        execution: Box<str>,
        name: Box<str>,
        value: Box<str>,
    },
    Directive {
        package_id: Box<str>,
        execution: Box<str>,
        index: usize,
        value: Box<str>,
    },
}

pub(crate) enum Package {
    Registry(package::Stored),
}

impl Recipe {
    pub(in crate::input) fn view(&self) -> View<'_, Package> {
        match self {
            Self::Workspace { roots } => View::Workspace { roots },
            Self::File {
                role,
                package_id,
                path,
            } => View::File {
                role: *role,
                package_id: package_id.as_deref(),
                path,
            },
            Self::Path {
                role,
                package_id,
                path,
            } => View::Path {
                role: *role,
                package_id: package_id.as_deref(),
                path,
            },
            Self::Directory {
                role,
                package_id,
                path,
            } => View::Directory {
                role: *role,
                package_id: package_id.as_deref(),
                path,
            },
            Self::DepInfo {
                package_id,
                path,
                package_root,
                out_directories,
            } => View::DepInfo {
                package_id,
                path,
                package_root,
                out_directories,
            },
            Self::SavedOutput {
                package_id,
                out_dir,
            } => View::SavedOutput {
                package_id,
                out_dir,
            },
            Self::Package(package) => View::Package(package),
            Self::Environment {
                package_id,
                execution,
                key,
                value,
            } => View::Environment {
                package_id,
                execution,
                key,
                value: value.as_deref(),
            },
            Self::EnvironmentProjection { schema, key, value } => View::EnvironmentProjection {
                schema: *schema,
                key,
                value: value.as_deref(),
            },
            Self::Fact {
                role,
                package_id,
                name,
                value,
            } => View::Fact {
                role: *role,
                package_id: package_id.as_deref(),
                name,
                value,
            },
            Self::BuildMessage {
                package_id,
                execution,
                name,
                value,
            } => View::BuildMessage {
                package_id,
                execution,
                name,
                value,
            },
            Self::Directive {
                package_id,
                execution,
                index,
                value,
            } => View::Directive {
                package_id,
                execution,
                index: *index,
                value,
            },
        }
    }

    pub(super) fn capture(recipe: proof::Recipe) -> Result<Self> {
        Ok(match recipe {
            proof::Recipe::Workspace { roots, .. } => Self::Workspace { roots },
            proof::Recipe::File {
                role,
                package_id,
                path,
            } => Self::File {
                role,
                package_id,
                path,
            },
            proof::Recipe::Path {
                role,
                package_id,
                path,
            } => Self::Path {
                role,
                package_id,
                path,
            },
            proof::Recipe::Directory {
                role,
                package_id,
                path,
            } => Self::Directory {
                role,
                package_id,
                path,
            },
            proof::Recipe::DepInfo {
                package_id,
                path,
                package_root,
                out_directories,
            } => Self::DepInfo {
                package_id,
                path,
                package_root,
                out_directories,
            },
            proof::Recipe::SavedOutput {
                package_id,
                out_dir,
            } => Self::SavedOutput {
                package_id,
                out_dir,
            },
            proof::Recipe::Package(package::Set::Registry(registry)) => {
                Self::Package(Package::Registry(registry.into_stored()))
            }
            proof::Recipe::Package(package::Set::Path(_) | package::Set::Git(_)) => {
                return Err(Error::IncompleteWitness)
            }
            proof::Recipe::Environment {
                package_id,
                execution,
                key,
                value,
            } => Self::Environment {
                package_id,
                execution,
                key,
                value,
            },
            proof::Recipe::EnvironmentProjection { schema, key, value } => {
                Self::EnvironmentProjection { schema, key, value }
            }
            proof::Recipe::Fact {
                role,
                package_id,
                name,
                value,
            } => Self::Fact {
                role,
                package_id,
                name,
                value,
            },
            proof::Recipe::BuildMessage {
                package_id,
                execution,
                name,
                value,
            } => Self::BuildMessage {
                package_id,
                execution,
                name,
                value,
            },
            proof::Recipe::Directive {
                package_id,
                execution,
                index,
                value,
            } => Self::Directive {
                package_id,
                execution,
                index,
                value,
            },
        })
    }
}

impl Writer<'_> {
    pub(super) fn recipe(&mut self, recipe: &Recipe) -> Result<()> {
        match recipe.view() {
            View::Workspace { roots } => {
                self.byte(11)?;
                self.count(roots.len())?;
                for path in roots {
                    self.path(path)?;
                }
                Ok(())
            }
            View::File {
                role,
                package_id,
                path,
            } => {
                self.byte(0)?;
                self.role(role)?;
                self.option(package_id)?;
                self.path(path)
            }
            View::Path {
                role,
                package_id,
                path,
            } => {
                self.byte(1)?;
                self.role(role)?;
                self.option(package_id)?;
                self.path(path)
            }
            View::Directory {
                role,
                package_id,
                path,
            } => {
                self.byte(2)?;
                self.role(role)?;
                self.option(package_id)?;
                self.path(path)
            }
            View::DepInfo {
                package_id,
                path,
                package_root,
                out_directories,
            } => {
                self.byte(3)?;
                self.text(package_id)?;
                self.path(path)?;
                self.path(package_root)?;
                self.count(out_directories.len())?;
                for path in out_directories {
                    self.path(path)?;
                }
                Ok(())
            }
            View::SavedOutput {
                package_id,
                out_dir,
            } => {
                self.byte(4)?;
                self.text(package_id)?;
                self.path(out_dir)
            }
            View::Package(package) => {
                self.byte(5)?;
                self.package(package)
            }
            View::Environment {
                package_id,
                execution,
                key,
                value,
            } => {
                self.byte(6)?;
                self.text(package_id)?;
                self.text(execution)?;
                self.text(key)?;
                self.option(value)
            }
            View::EnvironmentProjection { schema, key, value } => {
                self.byte(7)?;
                self.u32(schema)?;
                self.text(key)?;
                self.option(value)
            }
            View::Fact {
                role,
                package_id,
                name,
                value,
            } => {
                self.byte(8)?;
                self.role(role)?;
                self.option(package_id)?;
                self.text(name)?;
                self.text(value)
            }
            View::BuildMessage {
                package_id,
                execution,
                name,
                value,
            } => {
                self.byte(9)?;
                self.text(package_id)?;
                self.text(execution)?;
                self.text(name)?;
                self.text(value)
            }
            View::Directive {
                package_id,
                execution,
                index,
                value,
            } => {
                self.byte(10)?;
                self.text(package_id)?;
                self.text(execution)?;
                self.count(index)?;
                self.text(value)
            }
        }
    }

    fn package(&mut self, package: &Package) -> Result<()> {
        match package {
            Package::Registry(registry) => {
                self.byte(2)?;
                self.usage(registry.base_usage)?;
                self.usage(registry.usage)?;
                self.text(&registry.package_id)?;
                self.text(&registry.source)?;
                self.raw(&registry.checksum)?;
                self.path(&registry.root)?;
                self.path(&registry.marker)?;
                self.u32(package::REGISTRY_MARKER_SCHEMA)?;
                self.raw(&registry.marker_digest)
            }
        }
    }
}

impl Reader<'_, '_, '_> {
    pub(super) fn recipe(&mut self) -> Result<Recipe> {
        Ok(match self.byte()? {
            11 => {
                let count = self.count()?;
                if count > self.remaining() / 8 {
                    return Err(Error::Encoding("impossible workspace root count"));
                }
                let mut roots = self.budget.items(count)?;
                for _ in 0..count {
                    let path = self.path()?;
                    if roots.last().is_some_and(|previous| previous >= &path) {
                        return Err(Error::Encoding("workspace roots are not strictly ordered"));
                    }
                    roots.push(path);
                }
                Recipe::Workspace {
                    roots: self.budget.boxed(roots)?,
                }
            }
            0 => Recipe::File {
                role: self.role()?,
                package_id: self.option()?,
                path: self.path()?,
            },
            1 => Recipe::Path {
                role: self.role()?,
                package_id: self.option()?,
                path: self.path()?,
            },
            2 => Recipe::Directory {
                role: self.role()?,
                package_id: self.option()?,
                path: self.path()?,
            },
            3 => {
                let package_id = self.text()?;
                let path = self.path()?;
                let package_root = self.path()?;
                let count = self.count()?;
                if count > self.remaining() / 8 {
                    return Err(Error::Encoding("impossible OUT_DIR count"));
                }
                let mut paths = self.budget.items(count)?;
                for _ in 0..count {
                    paths.push(self.path()?);
                }
                Recipe::DepInfo {
                    package_id,
                    path,
                    package_root,
                    out_directories: self.budget.boxed(paths)?,
                }
            }
            4 => Recipe::SavedOutput {
                package_id: self.text()?,
                out_dir: self.path()?,
            },
            5 => Recipe::Package(self.package()?),
            6 => Recipe::Environment {
                package_id: self.text()?,
                execution: self.text()?,
                key: self.text()?,
                value: self.option()?,
            },
            7 => Recipe::EnvironmentProjection {
                schema: self.u32()?,
                key: self.text()?,
                value: self.option()?,
            },
            8 => Recipe::Fact {
                role: self.role()?,
                package_id: self.option()?,
                name: self.text()?,
                value: self.text()?,
            },
            9 => Recipe::BuildMessage {
                package_id: self.text()?,
                execution: self.text()?,
                name: self.text()?,
                value: self.text()?,
            },
            10 => Recipe::Directive {
                package_id: self.text()?,
                execution: self.text()?,
                index: self.length()?,
                value: self.text()?,
            },
            _ => return Err(Error::Encoding("unknown recipe tag")),
        })
    }

    fn package(&mut self) -> Result<Package> {
        match self.byte()? {
            1 | 3 => Err(Error::IncompleteWitness),
            2 => {
                let base_usage = self.usage()?;
                let usage = self.usage()?;
                if usage.items < base_usage.items || usage.bytes < base_usage.bytes {
                    return Err(Error::Encoding(
                        "registry usage precedes its transferred base",
                    ));
                }
                let package_id = self.text()?;
                let source = self.text()?;
                let checksum = self.digest()?;
                let root = self.path()?;
                let marker = self.path()?;
                if self.u32()? != package::REGISTRY_MARKER_SCHEMA {
                    return Err(Error::Encoding("unknown registry marker schema"));
                }
                let marker_digest = self.digest()?;
                Ok(Package::Registry(package::Stored {
                    base_usage,
                    usage,
                    package_id,
                    source,
                    checksum,
                    root,
                    marker,
                    marker_digest,
                }))
            }
            _ => Err(Error::Encoding("unknown package recipe tag")),
        }
    }
}

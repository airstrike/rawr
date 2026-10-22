//! Ordered normalization with externally owned stable target storage.
use super::{
    config,
    normalize::{self, all_targets, package, tail, validate},
    roots, Preparing, Warnings,
};
use crate::{
    cargo,
    input::membership::{
        host,
        manifest::{session::Session, storage::slots, Error, Step},
    },
};

mod contexts;
mod convert;
mod frames;
mod run;
mod selection;
pub(super) use selection::Package;
mod walk;
pub(super) use convert::{Converted, Failure, Output, Report};
pub(super) use frames::{Action, Cause, Continuation, Stack};
pub(super) use run::{run, Outcome};
pub(super) use walk::{walk, Work};

pub(super) enum Progress<'a, 'd, 'owner> {
    Patches {
        pending: normalize::Workspace<'a, 'd>,
        public_dependency: bool,
        errors: validate::Errors,
    },
    Package {
        pending: package::Workspace<'a, 'd>,
        public_dependency: bool,
        errors: validate::Errors,
    },
    Tail {
        owner: &'owner all_targets::Owner<'a, 'd>,
        warnings: Warnings,
        errors: validate::Errors,
        progress: Option<tail::Progress<'owner, 'd>>,
    },
}
/// Discovery inputs retained by a reached suspension, not a selected root.
pub(super) struct Request<'a, 'pointer> {
    pub(super) manifest: &'a std::path::Path,
    pub(super) explicit_root: Option<&'pointer str>,
}
/// A selected path still requires lookup/loading; candidates require full
/// manifest conversion before their workspace configuration can select a root.
pub(super) enum Search<'p, 'a, 'd> {
    Selected(super::discovery::Lookup<'a, 'd>),
    Candidates(super::discovery::Candidates<'p>),
}
impl<'p> Request<'p, '_> {
    pub(super) fn search<'a, 'd, 'image: 'p>(
        &self,
        home: &'p std::path::Path,
        roots: &roots::Roots<'a, 'd>,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Search<'p, 'a, 'd>> {
        if let Some(selected) = self.explicit_lookup(roots, session)? {
            return Ok(Search::Selected(selected));
        }
        match super::discovery::begin(self.manifest, home, roots, session)? {
            super::discovery::Discovery::Cached(path) => {
                super::discovery::lookup(path, roots, session).map(Search::Selected)
            }
            super::discovery::Discovery::Candidates(candidates) => {
                Ok(Search::Candidates(candidates))
            }
        }
    }
    pub(super) fn explicit_lookup<'a, 'd, A: rawr_toml::document::Admission>(
        &self,
        roots: &roots::Roots<'a, 'd>,
        session: &mut Session<'_, A>,
    ) -> Step<Option<super::discovery::Lookup<'a, 'd>>> {
        self.explicit_candidate(session)?
            .map(|candidate| super::discovery::lookup(candidate, roots, session))
            .transpose()
    }
    pub(super) fn explicit_candidate<A: rawr_toml::document::Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<Option<std::path::PathBuf>> {
        session.check()?;
        let Some(root) = self.explicit_root else {
            return Ok(None);
        };
        let Some(parent) = self.manifest.parent() else {
            return session.fail(Error::State);
        };
        let directory = super::path::join(parent, std::path::Path::new(root), session)?;
        let manifest = super::path::join(&directory, std::path::Path::new("Cargo.toml"), session)?;
        super::path::normalize(&manifest, session).map(Some)
    }
}
impl<'a, 'd> Progress<'a, 'd, '_> {
    pub(super) fn workspace_request(&self) -> Option<Request<'a, '_>> {
        match self {
            Self::Patches { pending, .. } => Some(Request {
                manifest: pending.manifest(),
                explicit_root: pending.explicit_root(),
            }),
            Self::Package { pending, .. } => Some(Request {
                manifest: pending.manifest(),
                explicit_root: pending.explicit_root(),
            }),
            Self::Tail {
                owner,
                progress: Some(tail::Progress::Workspace(_)),
                ..
            } => Some(Request {
                manifest: owner.manifest(),
                explicit_root: owner.explicit_root(),
            }),
            Self::Tail { .. } => None,
        }
    }
}
pub(super) fn start<'a: 'owner, 'd: 'a, 'owner, 'image: 'config, 'config>(
    preparing: Preparing<'a, 'd>,
    targets: &mut slots::Writer<'owner, all_targets::Owner<'a, 'd>>,
    config: &mut config::Owner<'config, 'image>,
    public_dependency: bool,
    limits: &cargo::Limits,
    session: &mut Session<'_, host::Host<'image, '_>>,
) -> Step<Result<Progress<'a, 'd, 'owner>, String>> {
    let progress = preparing.normalize_patches(config, limits, session)?;
    patches(
        progress,
        targets,
        config,
        public_dependency,
        validate::Errors::new(),
        limits,
        session,
    )
}
fn patches<'a: 'owner, 'd: 'a, 'owner, 'image: 'config, 'config>(
    progress: normalize::Progress<'a, 'd>,
    targets: &mut slots::Writer<'owner, all_targets::Owner<'a, 'd>>,
    config: &mut config::Owner<'config, 'image>,
    public_dependency: bool,
    errors: validate::Errors,
    limits: &cargo::Limits,
    session: &mut Session<'_, host::Host<'image, '_>>,
) -> Step<Result<Progress<'a, 'd, 'owner>, String>> {
    let patches = match progress {
        normalize::Progress::Patches(value) => value,
        normalize::Progress::Workspace(pending) => {
            return Ok(Ok(Progress::Patches {
                pending,
                public_dependency,
                errors,
            }))
        }
    };
    let progress = patches.normalize_package(session)?;
    package(
        progress,
        targets,
        config,
        public_dependency,
        errors,
        limits,
        session,
    )
}
fn package<'a: 'owner, 'd: 'a, 'owner, 'image: 'config, 'config>(
    progress: package::Progress<'a, 'd>,
    targets: &mut slots::Writer<'owner, all_targets::Owner<'a, 'd>>,
    config: &mut config::Owner<'config, 'image>,
    public_dependency: bool,
    mut errors: validate::Errors,
    limits: &cargo::Limits,
    session: &mut Session<'_, host::Host<'image, '_>>,
) -> Step<Result<Progress<'a, 'd, 'owner>, String>> {
    let package = match progress {
        package::Progress::Complete(value) => value,
        package::Progress::Workspace(pending) => {
            return Ok(Ok(Progress::Package {
                pending,
                public_dependency,
                errors,
            }))
        }
    };
    let library = package.normalize_library(session)?;
    let completed = match library.normalize_targets(&mut errors, session)? {
        Ok(value) => value,
        Err(error) => return Ok(Err(error)),
    };
    let (owner, mut warnings) = completed.into_tail();
    let owner = targets.push(owner, session)?;
    let progress =
        match owner.normalize(public_dependency, &mut warnings, config, limits, session)? {
            Ok(value) => value,
            Err(error) => return Ok(Err(error)),
        };
    Ok(Ok(Progress::Tail {
        owner,
        warnings,
        errors,
        progress,
    }))
}

impl<'a: 'owner, 'd: 'a, 'owner> Progress<'a, 'd, 'owner> {
    /// The caller must supply the root selected by the cache branch or a
    /// successful recursive load, never a bare Reader::prepare result.
    pub(super) fn resume_declared<'image: 'config, 'config>(
        self,
        root: roots::Root<'a, 'd>,
        targets: &mut slots::Writer<'owner, all_targets::Owner<'a, 'd>>,
        config: &mut config::Owner<'config, 'image>,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Result<Self, String>> {
        session.check()?;
        match self {
            Self::Patches {
                pending,
                public_dependency,
                errors,
            } => {
                let progress = pending.resume_declared(root, config, limits, session)?;
                patches(
                    progress,
                    targets,
                    config,
                    public_dependency,
                    errors,
                    limits,
                    session,
                )
            }
            Self::Package {
                pending,
                public_dependency,
                errors,
            } => {
                let progress = pending.resume_declared(root, session)?;
                package(
                    progress,
                    targets,
                    config,
                    public_dependency,
                    errors,
                    limits,
                    session,
                )
            }
            Self::Tail {
                owner,
                mut warnings,
                errors,
                progress: Some(tail::Progress::Workspace(pending)),
            } => {
                let progress =
                    pending.resume_declared(root, &mut warnings, config, limits, session)?;
                Ok(Ok(Self::Tail {
                    owner,
                    warnings,
                    errors,
                    progress: Some(progress),
                }))
            }
            Self::Tail { .. } => session.fail(Error::State),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{load::discovery::Lookup, reference};
    use std::path::Path;
    #[test]
    fn explicit_workspace_lookup_uses_directory_key_and_preserves_load_path() {
        for key in [None, Some("/root/Cargo.toml"), Some("/root")] {
            let mut account = reference::Account;
            let mut session = Session::new(&mut account);
            let mut roots = roots::Roots::new();
            if let Some(key) = key {
                roots
                    .insert(
                        Path::new(key),
                        roots::Root {
                            directory: Path::new("/root"),
                            declaration: None,
                        },
                        &mut session,
                    )
                    .unwrap();
            }
            let request = Request {
                manifest: Path::new("/p/Cargo.toml"),
                explicit_root: Some("../root"),
            };
            let result = request
                .explicit_lookup(&roots, &mut session)
                .unwrap()
                .unwrap();
            match (key, result) {
                (Some("/root"), Lookup::Cached(root)) => {
                    assert_eq!(root.directory, Path::new("/root"))
                }
                (None | Some("/root/Cargo.toml"), Lookup::Load(path)) => {
                    assert_eq!(path, Path::new("/root/Cargo.toml"))
                }
                _ => panic!("wrong cache branch"),
            }
        }
    }
}

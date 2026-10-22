//! Conversion dispatch after normalization, with a separate stable tail tier.
use super::*;
use crate::input::membership::manifest::{
    load::convert::{self, package, source},
    spec::Spec,
};

pub(in super::super) enum Output<'a, 'd, 's> {
    Real(Real<'a, 'd, 's>),
    Virtual(convert::workspace::Complete<'a, 's>),
    PendingReal(convert::real::Conversion<'a, 'd, 's>),
    PendingVirtual,
}
/// Completed conversion only; loader ownership and report attachment still follow.
pub(in super::super) struct Real<'a, 'd, 's> {
    pub(in super::super) targets: convert::Targets<'a, 'd>,
    pub(in super::super) dependencies: convert::dependencies::all::Complete<'a, 's>,
}
impl<'a, 'd, 's> Output<'a, 'd, 's> {
    fn real(value: convert::real::Conversion<'a, 'd, 's>) -> Self {
        let convert::real::Conversion {
            header,
            targets,
            dependencies,
        } = value;
        match dependencies {
            convert::dependencies::all::Progress::Complete(dependencies) => Self::Real(Real {
                targets,
                dependencies,
            }),
            dependencies => Self::PendingReal(convert::real::Conversion {
                header,
                targets,
                dependencies,
            }),
        }
    }
    fn virtual_manifest(value: convert::workspace::Progress<'a, 's>) -> Self {
        match value {
            convert::workspace::Progress::Complete(value) => Self::Virtual(value),
            _ => Self::PendingVirtual,
        }
    }
}
pub(in super::super) enum Failure {
    Real(convert::real::Failure),
    Virtual(convert::replace::list::Failure),
}
pub(in super::super) struct Report<'a, 'd, 'owner, 'tail, 's> {
    pub(in super::super) owner: &'owner all_targets::Owner<'a, 'd>,
    /// The caller's identity, including virtual manifests and suspended work.
    pub(in super::super) source: source::Id<'s>,
    pub(in super::super) warnings: Warnings,
    pub(in super::super) errors: validate::Errors,
    pub(in super::super) result: Result<Output<'tail, 'd, 's>, Failure>,
}
pub(in super::super) enum Converted<'a, 'd, 's> {
    Real(Real<'a, 'd, 's>),
    Virtual(convert::workspace::Complete<'a, 's>),
}
/// Successful conversion with Cargo's final diagnostic attachment completed.
pub(in super::super) struct Finished<'a, 'd, 'owner, 'tail, 's> {
    pub(in super::super) owner: &'owner all_targets::Owner<'a, 'd>,
    pub(in super::super) source: source::Id<'s>,
    pub(in super::super) output: Converted<'tail, 'd, 's>,
    pub(in super::super) reports: super::super::reports::Attached,
}
impl<'a, 'd> Finished<'a, 'd, '_, '_, '_> {
    /// Cargo's post-load inheritance branch. This method is unavailable until
    /// all conversion and ordinary/critical diagnostic attachment has succeeded.
    pub(in super::super) fn inheritable<A: rawr_toml::document::Admission>(
        &self,
        roots: &mut roots::Roots<'a, 'd>,
        session: &mut Session<'_, A>,
    ) -> Step<Result<roots::Root<'a, 'd>, String>> {
        session.check()?;
        let super::super::workspace::Configuration::Root {
            directory,
            declaration,
        } = self.owner.workspace()
        else {
            return super::super::formatted(
                format_args!(
                    "root of a workspace inferred but wasn't a root: {}",
                    self.owner.manifest().display()
                ),
                session,
            )
            .map(Err);
        };
        let root = roots::Root {
            directory,
            declaration: *declaration,
        };
        // Pinned inheritable_from_path inserts the manifest-file key here.
        roots.insert(self.owner.manifest(), root, session)?;
        Ok(Ok(root))
    }
}
impl<'a, 'd, 'owner, 'tail, 's> Report<'a, 'd, 'owner, 'tail, 's> {
    /// Noncompleted reports are returned intact, without attachment work.
    pub(in super::super) fn finish<A: rawr_toml::document::Admission>(
        self,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Finished<'a, 'd, 'owner, 'tail, 's>, Self>> {
        session.check()?;
        let output = match self.result {
            Ok(Output::Real(value)) => Converted::Real(value),
            Ok(Output::Virtual(value)) => Converted::Virtual(value),
            result => return Ok(Err(Self { result, ..self })),
        };
        let reports = super::super::reports::Attached::attach(self.warnings, self.errors, session)?;
        Ok(Ok(Finished {
            owner: self.owner,
            source: self.source,
            output,
            reports,
        }))
    }
}
/// A borrow refined by completed conversion. It selects a pathname only;
/// recursive loading and final manifest publication remain separate operations.
pub(in super::super) struct Candidate<'r, 'a, 'd> {
    owner: &'r all_targets::Owner<'a, 'd>,
}
impl<'a, 'd> Report<'a, 'd, '_, '_, '_> {
    pub(in super::super) fn candidate(&self) -> Option<Candidate<'_, 'a, 'd>> {
        match &self.result {
            Ok(Output::Real(_) | Output::Virtual(_)) => Some(Candidate { owner: self.owner }),
            Ok(Output::PendingReal(_) | Output::PendingVirtual) | Err(_) => None,
        }
    }
}
impl Candidate<'_, '_, '_> {
    pub(in super::super) fn selected_root<A: rawr_toml::document::Admission>(
        &self,
        look_from: &std::path::Path,
        session: &mut Session<'_, A>,
    ) -> Step<Option<std::path::PathBuf>> {
        self.owner
            .workspace()
            .selected_root(self.owner.manifest(), look_from, session)
    }
}
impl<'a: 'owner, 'd: 'a, 'owner> Progress<'a, 'd, 'owner> {
    /// Pending normalization must first be resumed with its selected workspace.
    /// Returned conversion may itself suspend on native dependency operations.
    pub(in super::super) fn convert<'tail, 's: 'tail, 'image: 's>(
        self,
        tails: &mut slots::Writer<'tail, tail::Complete<'owner, 'd>>,
        specs: &mut slots::Writer<'tail, Spec>,
        packages: &mut package::Owner<'tail, 's>,
        parent: source::Id<'s>,
        sources: &mut source::Owner<'s>,
        config: &mut config::Owner<'s, 'image>,
        bindeps: bool,
        json_target_spec: bool,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Report<'a, 'd, 'owner, 'tail, 's>>
    where
        'owner: 'tail,
    {
        session.check()?;
        let Self::Tail {
            owner,
            mut warnings,
            errors,
            progress,
        } = self
        else {
            return session.fail(Error::State);
        };
        let result = match progress {
            Some(tail::Progress::Complete(tail)) => {
                let tail = tails.push(tail, session)?;
                owner
                    .convert_real(
                        tail,
                        specs,
                        packages,
                        parent,
                        sources,
                        config,
                        bindeps,
                        json_target_spec,
                        limits,
                        &mut warnings,
                        session,
                    )?
                    .map(Output::real)
                    .map_err(Failure::Real)
            }
            None => owner
                .convert_virtual(
                    specs,
                    parent,
                    sources,
                    config,
                    bindeps,
                    json_target_spec,
                    limits,
                    &mut warnings,
                    session,
                )?
                .map(Output::virtual_manifest)
                .map_err(Failure::Virtual),
            Some(tail::Progress::Workspace(_)) => return session.fail(Error::State),
        };
        Ok(Report {
            owner,
            source: parent,
            warnings,
            errors,
            result,
        })
    }
}

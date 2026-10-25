//! Consecutive real-manifest conversion from a completed normalization tail.
//! Dependency/native pending state remains explicit; this is not Loaded authority.
use super::{dependencies::all, package, replace, source, Header, Targets};
use crate::{
    cargo,
    input::membership::{
        host,
        manifest::{
            load::{
                config,
                normalize::{all_targets::Owner, tail},
                Warnings,
            },
            session::Session,
            spec::Spec,
            storage::slots,
            Step,
        },
    },
};

pub(in super::super) enum Failure {
    Header,
    Target,
    Validation(String),
    Dependency(replace::list::Failure),
}
pub(in super::super) struct Conversion<'a, 'd, 's> {
    pub(in super::super) header: Header,
    pub(in super::super) targets: Targets<'a, 'd>,
    pub(in super::super) dependencies: all::Progress<'a, 'd, 's>,
}
impl<'a, 'd> Owner<'a, 'd> {
    pub(in super::super) fn convert_real<'b, 's: 'b, 'image: 's>(
        &'b self,
        tail: &'b tail::Complete<'b, 'd>,
        specs: &mut slots::Writer<'b, Spec>,
        packages: &mut package::Owner<'b, 's>,
        parent: source::Id<'s>,
        sources: &mut source::Owner<'s>,
        config: &mut config::Owner<'s, 'image>,
        bindeps: bool,
        json_target_spec: bool,
        limits: &cargo::Limits,
        warnings: &mut Warnings,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Result<Conversion<'b, 'd, 's>, Failure>> {
        let header = match self.conversion_header(warnings, session)? {
            Ok(value) => value,
            Err(_) => return Ok(Err(Failure::Header)),
        };
        let targets = match self.convert_targets(&header, tail, warnings, session)? {
            Ok(value) => value,
            Err(_) => return Ok(Err(Failure::Target)),
        };
        if let Err(error) = self.validate_targets(&targets, warnings, session)? {
            return Ok(Err(Failure::Validation(error)));
        }
        if let Err(error) = self.validate_dependencies(warnings, session)? {
            return Ok(Err(Failure::Validation(error)));
        }
        let dependencies = match self.convert_dependencies(
            tail,
            &targets,
            specs,
            packages,
            parent,
            sources,
            config,
            bindeps,
            json_target_spec,
            limits,
            warnings,
            session,
        )? {
            Ok(value) => value,
            Err(error) => return Ok(Err(Failure::Dependency(error))),
        };
        Ok(Ok(Conversion {
            header,
            targets,
            dependencies,
        }))
    }
}

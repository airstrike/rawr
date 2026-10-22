// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../../path/LICENSE-MIT and ../../path/LICENSE-APACHE.
//! Ordered replacement conversion with scoped, stable spec ownership.
use super::{dependency, platform, source, InvalidKey};
use crate::{
    cargo,
    input::membership::{
        host,
        manifest::{
            load::{config, Warnings},
            schema::manifest,
            session::Session,
            spec::Spec,
            storage::{self, slots},
            Error, Step,
        },
    },
};
use std::path::Path;

pub(in super::super::super) enum Failure {
    Key(Box<InvalidKey>),
    Conversion(String),
    MissingFeatureDependency(super::super::summary::Missing),
    FeatureGate(super::super::finish::Failure),
}
pub(in super::super::super) struct Entry<'a, 'p, 's> {
    pub(in super::super::super) spec: &'a Spec,
    pub(in super::super::super) dependency: dependency::Converted<'p, 's>,
}
pub(in super::super::super) enum Progress<'a, 'p, 's> {
    Complete(storage::Buffer<Entry<'a, 'p, 's>>),
    Canonical(Pending<'a, 'p, 's>),
}
/// No replay or completion API exists before the native-evidence connection.
pub(in super::super::super) struct Pending<'a, 'p, 's> {
    completed: storage::Buffer<Entry<'a, 'p, 's>>,
    spec: &'a Spec,
    dependency: dependency::Pending<'a, 'p, 's>,
}
impl Pending<'_, '_, '_> {
    pub(in super::super::super) fn completed_count(&self) -> usize {
        self.completed.as_slice().len()
    }
    pub(in super::super::super) fn spelling(&self) -> &str {
        self.dependency.spelling()
    }
    pub(in super::super::super) fn spec(&self) -> &Spec {
        self.spec
    }
}

pub(in super::super::super) fn convert<'a, 'd: 'a, 'p, 's, 'image: 's>(
    original: &'a manifest::Original<'d>,
    specs: &mut slots::Writer<'a, Spec>,
    manifest: &Path,
    parent: source::Id<'s>,
    sources: &mut source::Owner<'s>,
    config: &mut config::Owner<'s, 'image>,
    platform: Option<&platform::Platform<'p>>,
    bindeps: bool,
    json_target_spec: bool,
    limits: &cargo::Limits,
    warnings: &mut Warnings,
    session: &mut Session<'_, host::Host<'image, '_>>,
) -> Step<Result<Progress<'a, 'p, 's>, Failure>> {
    if let Err(error) = super::check_tables(original, session)? {
        return Ok(Err(Failure::Conversion(error)));
    }
    let mut completed = storage::Buffer::new();
    let map = match original.get(manifest::Field::Replace) {
        None => return Ok(Ok(Progress::Complete(completed))),
        Some(manifest::Value::Replace(map)) => map,
        _ => return session.fail(Error::State),
    };
    for declaration in map.entries() {
        session.items(1)?;
        let spec = match super::key(
            declaration.origin().text.as_str(),
            declaration.origin().span,
            session,
        )? {
            Ok(spec) => specs.push(spec, session)?,
            Err(error) => return Ok(Err(Failure::Key(Box::new(error)))),
        };
        match super::entry(
            spec,
            declaration.value(),
            manifest,
            parent,
            sources,
            config,
            platform,
            bindeps,
            json_target_spec,
            limits,
            warnings,
            session,
        )? {
            Err(error) => return Ok(Err(Failure::Conversion(error))),
            Ok(dependency::Progress::Complete(dependency)) => {
                completed.push(Entry { spec, dependency }, session)?;
            }
            Ok(dependency::Progress::Canonical(dependency)) => {
                return Ok(Ok(Progress::Canonical(Pending {
                    completed,
                    spec,
                    dependency,
                })))
            }
        }
    }
    Ok(Ok(Progress::Complete(completed)))
}

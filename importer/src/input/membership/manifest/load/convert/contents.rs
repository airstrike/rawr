//! Owned package fields retained after default and forced target construction.
use super::{metadata, value};
use crate::input::membership::manifest::{
    load::normalize::package,
    schema::{package::Field, primitive},
    session::Session,
    storage, Error, Step,
};
use rawr_toml::document;

pub(in super::super) struct Owned {
    pub(in super::super) include: storage::Buffer<String>,
    pub(in super::super) exclude: storage::Buffer<String>,
    pub(in super::super) links: Option<String>,
    pub(in super::super) metadata: Option<value::Owned>,
    pub(in super::super) teapot: Option<bool>,
    pub(in super::super) default_run: Option<String>,
    pub(in super::super) metabuild: Option<storage::Buffer<String>>,
}
pub(in super::super) fn retain<A: document::Admission>(
    package: &package::View<'_, '_>,
    session: &mut Session<'_, A>,
) -> Step<Owned> {
    session.check()?;
    let include = metadata::strings(package, Field::Include, session)?;
    let exclude = metadata::strings(package, Field::Exclude, session)?;
    let links = metadata::text(package, Field::Links, session)?;
    session.items(1)?;
    let metadata = match package.get(Field::Metadata) {
        Some(package::Value::Metadata(value)) => Some(value::copy(value, session)?),
        None => None,
        _ => return session.fail(Error::State),
    };
    session.items(1)?;
    let teapot = match package.get(Field::ImATeapot) {
        Some(package::Value::Original(primitive::Value::Boolean(value))) => Some(*value),
        None => None,
        _ => return session.fail(Error::State),
    };
    let default_run = metadata::text(package, Field::DefaultRun, session)?;
    session.items(1)?;
    let metabuild = if package.get(Field::Metabuild).is_some() {
        Some(metadata::strings(package, Field::Metabuild, session)?)
    } else {
        None
    };
    Ok(Owned {
        include,
        exclude,
        links,
        metadata,
        teapot,
        default_run,
        metabuild,
    })
}

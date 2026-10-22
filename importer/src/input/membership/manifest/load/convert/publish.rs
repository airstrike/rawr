// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Normalized publish policy and version prerequisite, before PackageId.
use crate::input::membership::manifest::{
    load::normalize::{package, target},
    schema::{package::Field, primitive},
    session::Session,
    storage, Error, Step,
};
use rawr_toml::document;

pub(in super::super) struct Policy {
    pub(in super::super) registries: Option<storage::Buffer<String>>,
}

pub(in super::super) fn convert<A: document::Admission>(
    package: &package::View<'_, '_>,
    session: &mut Session<'_, A>,
) -> Step<Result<Policy, String>> {
    session.items(1)?;
    let version = match package.get(Field::Version) {
        None => false,
        Some(package::Value::Original(primitive::Value::Version(_))) => true,
        _ => return session.fail(Error::State),
    };
    session.items(1)?;
    let registries = match package.get(Field::Publish) {
        Some(package::Value::Original(primitive::Value::StringsOrBool(
            primitive::StringsOrBool::Strings(values),
        ))) => {
            let mut copied = storage::Buffer::new();
            for value in values.as_slice() {
                session.items(1)?;
                let value = target::formatted(format_args!("{}", value.as_str()), session)?;
                session.items(copied.len())?;
                copied.push(value, session)?;
            }
            Some(copied)
        }
        Some(package::Value::Original(primitive::Value::StringsOrBool(
            primitive::StringsOrBool::Boolean(false),
        ))) => Some(storage::Buffer::new()),
        Some(package::Value::Original(primitive::Value::StringsOrBool(
            primitive::StringsOrBool::Boolean(true),
        ))) => None,
        None => (!version).then(storage::Buffer::new),
        _ => return session.fail(Error::State),
    };
    if !version
        && !registries
            .as_ref()
            .is_some_and(|values| values.as_slice().is_empty())
    {
        return target::formatted(
            format_args!("`package.publish` requires `package.version` be specified"),
            session,
        )
        .map(Err);
    }
    Ok(Ok(Policy { registries }))
}

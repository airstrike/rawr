// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Owned ManifestMetadata from normalized fields; no filesystem operations.
use crate::input::membership::manifest::{
    load::normalize::{library, package, target},
    scalar,
    schema::{manifest, package::Field, primitive},
    session::Session,
    storage, Error, Step,
};
use rawr_toml::document;

pub(in super::super) struct Metadata {
    pub(in super::super) description: Option<String>,
    pub(in super::super) homepage: Option<String>,
    pub(in super::super) documentation: Option<String>,
    pub(in super::super) readme: Option<String>,
    pub(in super::super) authors: storage::Buffer<String>,
    pub(in super::super) license: Option<String>,
    pub(in super::super) license_file: Option<String>,
    pub(in super::super) repository: Option<String>,
    pub(in super::super) keywords: storage::Buffer<String>,
    pub(in super::super) categories: storage::Buffer<String>,
    pub(in super::super) badges: storage::Buffer<Badge>,
    pub(in super::super) links: Option<String>,
    pub(in super::super) rust_version: Option<scalar::Rust>,
}
pub(in super::super) struct Badge {
    pub(in super::super) name: String,
    pub(in super::super) values: storage::Buffer<(String, String)>,
}

pub(in super::super) fn retain<A: document::Admission>(
    package: &package::View<'_, '_>,
    badges: Option<&manifest::Value<'_>>,
    session: &mut Session<'_, A>,
) -> Step<Metadata> {
    session.check()?;
    let description = text(package, Field::Description, session)?;
    let homepage = text(package, Field::Homepage, session)?;
    let documentation = text(package, Field::Documentation, session)?;
    let readme = text(package, Field::Readme, session)?;
    let authors = strings(package, Field::Authors, session)?;
    let license = text(package, Field::License, session)?;
    let license_file = text(package, Field::LicenseFile, session)?;
    let repository = text(package, Field::Repository, session)?;
    let keywords = strings(package, Field::Keywords, session)?;
    let categories = strings(package, Field::Categories, session)?;
    let mut copied = storage::Buffer::new();
    match badges {
        Some(manifest::Value::Primitive(primitive::Value::Badges(badges))) => {
            for badge in badges.entries() {
                session.items(1)?;
                let name = own(badge.key().text.as_str(), session)?;
                let mut values = storage::Buffer::new();
                for pair in badge.values() {
                    session.items(1)?;
                    let key = own(pair.key().text.as_str(), session)?;
                    let value = own(pair.text().as_str(), session)?;
                    session.items(values.len())?;
                    values.push((key, value), session)?;
                }
                session.items(copied.len())?;
                copied.push(Badge { name, values }, session)?;
            }
        }
        None => {}
        _ => return session.fail(Error::State),
    }
    let links = text(package, Field::Links, session)?;
    session.items(1)?;
    let rust_version = match package.get(Field::RustVersion) {
        Some(package::Value::Original(primitive::Value::Rust(value))) => Some(*value),
        None => None,
        _ => return session.fail(Error::State),
    };
    Ok(Metadata {
        description,
        homepage,
        documentation,
        readme,
        authors,
        license,
        license_file,
        repository,
        keywords,
        categories,
        badges: copied,
        links,
        rust_version,
    })
}

pub(super) fn text<A: document::Admission>(
    package: &package::View<'_, '_>,
    field: Field,
    session: &mut Session<'_, A>,
) -> Step<Option<String>> {
    session.items(1)?;
    match package.get(field) {
        None => Ok(None),
        Some(package::Value::Boolean(false)) if field == Field::Readme => Ok(None),
        Some(value) => match library::text(value) {
            Some(value) => own(value, session).map(Some),
            None => session.fail(Error::State),
        },
    }
}
pub(super) fn strings<A: document::Admission>(
    package: &package::View<'_, '_>,
    field: Field,
    session: &mut Session<'_, A>,
) -> Step<storage::Buffer<String>> {
    session.items(1)?;
    let mut copied = storage::Buffer::new();
    match package.get(field) {
        None => {}
        Some(package::Value::Original(primitive::Value::Strings(values))) => {
            for value in values.as_slice() {
                let value = own(value.as_str(), session)?;
                session.items(copied.len())?;
                copied.push(value, session)?;
            }
        }
        _ => return session.fail(Error::State),
    }
    Ok(copied)
}
fn own<A: document::Admission>(value: &str, session: &mut Session<'_, A>) -> Step<String> {
    session.string(value.len())?;
    target::formatted(format_args!("{value}"), session)
}

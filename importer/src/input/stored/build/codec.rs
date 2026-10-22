//! Canonical historical Build data, without package acquisition capability.

use crate::input::build;

use super::super::codec::Writer;
use super::super::{Budget, Error, Result};

const MAGIC: &[u8] = b"rawr-retained-build\0\x01";

impl build::Retained {
    /// Persist only the drained observations; decoding cannot reacquire packages.
    #[cfg(test)]
    pub(crate) fn encode(&self, budget: &mut Budget<'_>) -> Result<Vec<u8>> {
        budget.check()?;
        super::super::codec::encode(budget, |writer| self.write(writer))
    }

    pub(in crate::input::stored) fn write(&self, writer: &mut Writer<'_>) -> Result<()> {
        if !self.build.package_files.is_empty() {
            return Err(Error::Encoding(
                "retained Build still owns package file sets",
            ));
        }

        writer.raw(MAGIC)?;
        writer.text(crate::cargo::toolchain::supported_cargo_commit())?;
        let evidence = &self.build.evidence;
        writer.usage(evidence.usage)?;
        writer.count(evidence.packages.len())?;
        for (id, package) in &evidence.packages {
            writer.text(id)?;
            writer.path(&package.manifest)?;
            writer.path(&package.root)?;
            match &package.build_script {
                None => writer.byte(0)?,
                Some(path) => {
                    writer.byte(1)?;
                    writer.path(path)?;
                }
            }
        }
        writer.count(evidence.scripts.len())?;
        for script in &evidence.scripts {
            writer.text(&script.package_id)?;
            writer.path(&script.out_dir)?;
            strings(writer, &script.cfg)?;
            writer.count(script.environment.len())?;
            for (name, value) in &script.environment {
                writer.text(name)?;
                writer.text(value)?;
            }
        }
        writer.count(evidence.artifacts.len())?;
        for artifact in &evidence.artifacts {
            writer.text(&artifact.package_id)?;
            writer.count(artifact.outputs.len())?;
            for output in &artifact.outputs {
                writer.path(output)?;
            }
            writer.path(&artifact.dep_info)?;
            let key = &artifact.key;
            writer.text(&key.package_id)?;
            writer.path(&key.target)?;
            strings(writer, &key.target_kinds)?;
            strings(writer, &key.crate_types)?;
            writer.text(&key.crate_name)?;
            strings(writer, &key.features)?;
            writer.bytes(artifact.profile.as_bytes())?;
            writer.byte(u8::from(artifact.fresh))?;
        }
        writer.count(evidence.environment.len())?;
        for (name, value) in &evidence.environment {
            writer.bytes(name.as_encoded_bytes())?;
            writer.bytes(value.as_encoded_bytes())?;
        }
        Ok(())
    }
}

fn strings(writer: &mut Writer<'_>, values: &[Box<str>]) -> Result<()> {
    writer.count(values.len())?;
    for value in values {
        writer.text(value)?;
    }
    Ok(())
}

mod correlate;
mod profiles;

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use super::super::codec::Reader;
use crate::cargo::unit;

impl build::Retained {
    /// Restore historical observations against the caller's freshly parsed graph.
    /// The complete input and owner replay still determines current authority.
    pub(crate) fn decode(
        bytes: &[u8],
        graph: &unit::BuildGraph,
        command_build: &std::path::Path,
        target: &str,
        budget: &mut Budget<'_>,
    ) -> Result<Self> {
        budget.check()?;
        budget.wire_section(bytes)?;
        let profiles = profiles::Profiles::bind(graph, budget)?;
        let mut reader = Reader::new(bytes, budget);
        if reader.take(MAGIC.len())? != MAGIC
            || reader.bytes()? != crate::cargo::toolchain::supported_cargo_commit().as_bytes()
        {
            return Err(Error::Encoding(
                "unknown retained Build schema or Cargo pin",
            ));
        }
        let usage = reader.usage()?;
        let package_limit = reader.budget.limits.packages;
        let packages = list(&mut reader, package_limit, |reader| {
            let id = reader.text()?;
            let manifest = reader.path()?;
            let root = reader.path()?;
            let build_script = match reader.byte()? {
                0 => None,
                1 => Some(reader.path()?),
                _ => return Err(Error::Encoding("unknown Build script path tag")),
            };
            Ok((
                id,
                build::Package {
                    manifest,
                    root,
                    build_script,
                },
            ))
        })?;
        let packages = build::packages::Table::from_decoded(packages)?;
        let unit_limit = reader.budget.limits.units;
        let mut scripts = list(&mut reader, unit_limit, |reader| {
            let package_id = reader.text()?;
            let out_dir = reader.path()?;
            let cfg = texts(reader)?;
            let environment = list(reader, reader.budget.limits.items, |reader| {
                Ok((reader.text()?, reader.text()?))
            })?;
            if !packages.contains_key(&package_id) {
                return Err(Error::Encoding("script Build package is absent"));
            }
            Ok(build::Script {
                compiler: None,
                package_id,
                out_dir,
                cfg,
                environment,
            })
        })?;
        let artifacts = list(&mut reader, unit_limit, |reader| {
            let package_id = reader.text()?;
            let outputs = list(reader, reader.budget.limits.items, |reader| reader.path())?;
            let dep_info = reader.path()?;
            let key = build::Key {
                package_id: reader.text()?,
                target: reader.path()?,
                target_kinds: texts(reader)?,
                crate_types: texts(reader)?,
                crate_name: reader.text()?,
                features: texts(reader)?,
            };
            let bytes = reader.bytes()?;
            let profile = profiles.decode(bytes, reader.budget)?;
            let fresh = match reader.byte()? {
                0 => false,
                1 => true,
                _ => return Err(Error::Encoding("unknown Build freshness tag")),
            };
            if package_id != key.package_id || !packages.contains_key(&package_id) {
                return Err(Error::Encoding(
                    "artifact Build package association differs",
                ));
            }
            Ok(build::Artifact {
                package_id,
                outputs,
                dep_info,
                key,
                profile,
                fresh,
            })
        })?;
        let environment_limit = reader.budget.limits.items;
        let environment = list(&mut reader, environment_limit, |reader| {
            // Path decoding preserves native bytes; these values are not filesystem paths.
            let name: PathBuf = reader.path()?;
            let value: PathBuf = reader.path()?;
            Ok((OsString::from(name), OsString::from(value)))
        })?;
        reader.finish()?;
        let limits = budget.limits;
        correlate::require(graph.artifacts(), &artifacts, budget)?;
        // The existing script join owns two exact arrays of borrowed string slices.
        let count = graph
            .scripts()
            .len()
            .checked_add(scripts.len())
            .ok_or_else(|| budget.overflow())?;
        let bytes = count
            .checked_mul(std::mem::size_of::<&str>())
            .ok_or_else(|| budget.overflow())?;
        budget.storage(count, bytes)?;
        crate::input::correlate_scripts_with(
            graph.scripts(),
            &scripts,
            &limits,
            |code, _| Error::Encoding(code),
            Error::Allocation,
        )
        .map_err(|error| budget.failure(error))?;
        build::scripts::bind(
            graph,
            &mut scripts,
            &artifacts,
            command_build,
            target,
            budget,
        )?;
        Ok(Self {
            build: build::Build {
                evidence: build::Evidence {
                    usage,
                    packages,
                    scripts,
                    artifacts,
                    environment,
                },
                package_files: BTreeMap::new(),
            },
        })
    }
}

fn list<T>(
    reader: &mut Reader<'_, '_, '_>,
    limit: usize,
    mut decode: impl FnMut(&mut Reader<'_, '_, '_>) -> Result<T>,
) -> Result<Box<[T]>> {
    let count = reader.count()?;
    reader
        .budget
        .limited("retained Build section items", limit, count)?;
    if count > reader.remaining() {
        return Err(Error::Encoding("impossible retained Build count"));
    }
    let mut values = reader.budget.items(count)?;
    for _ in 0..count {
        values.push(decode(reader)?);
    }
    reader.budget.boxed(values)
}

fn texts(reader: &mut Reader<'_, '_, '_>) -> Result<Box<[Box<str>]>> {
    list(reader, reader.budget.limits.items, |reader| reader.text())
}

#[cfg(test)]
mod tests;

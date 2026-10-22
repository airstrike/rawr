//! Cargo's ordered archive projection; see provenance.json for exact lineage.
//!
//! Generated rows retain their kind and occupancy, without their virtual value
//! bytes. `Cargo.toml.orig` retains its exact physical manifest origin.

use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt as _;
use std::path;

use super::{host, observed, Budget, Error, Result};

/// Caller-owned selection and normalized manifest data, with no proof authority.
pub(super) struct Inputs<'input, B = &'input [&'input [u8]]> {
    pub(super) root: &'input [u8],
    pub(super) manifest: &'input [u8],
    pub(super) selected: &'input [&'input [u8]],
    pub(super) license: Option<&'input [u8]>,
    pub(super) readme: Option<&'input [u8]>,
    pub(super) build_scripts: B,
    pub(super) include_lockfile: bool,
    pub(super) vcs: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Generated {
    Manifest,
    Lockfile,
    VcsInfo,
}

#[derive(Debug, Eq, PartialEq)]
enum Contents {
    Copy(Vec<u8>),
    Generate(Generated),
}

#[derive(Debug, Eq, PartialEq)]
pub(super) struct Row {
    archive: Vec<u8>,
    contents: Contents,
}

/// The complete archive projection of supplied data, not a complete selector.
#[derive(Debug, Eq, PartialEq)]
pub(super) struct Projection {
    rows: Vec<Row>,
}

#[derive(Debug, thiserror::Error)]
pub(super) enum Rejection {
    #[error("selected source is outside the package root")]
    OutsideRoot,
    #[error("source path is not Unicode")]
    Unicode,
    #[error("source filename contains forbidden character {0}")]
    Filename(char),
    #[error("selected source uses a reserved archive filename")]
    Reserved,
    #[error("declared archive input is not a file: license={license}, readme={readme}")]
    Missing { license: bool, readme: bool },
    #[error("custom build source is absent or outside the package root")]
    BuildScript,
    #[error("external archive input has no basename")]
    Basename,
}

impl Projection {
    pub(super) fn from_inputs(input: Inputs<'_>, host: &mut host::Host<'_, '_>) -> Result<Self> {
        Self::from_iter(input, host)
    }
    pub(super) fn from_iter<B>(input: Inputs<'_, B>, host: &mut host::Host<'_, '_>) -> Result<Self>
    where
        B: IntoIterator,
        B::Item: AsRef<[u8]>,
    {
        let root = native(input.root);
        let mut rows = Vec::new();
        for selected in input.selected {
            let relative = native(selected)
                .strip_prefix(root)
                .map_err(|_| Rejection::OutsideRoot)?;
            filename(relative)?;
            let text = relative.to_str().ok_or(Rejection::Unicode)?;
            match text {
                "Cargo.lock" => continue,
                ".cargo_vcs_info.json" | "Cargo.toml.orig" => {
                    return Err(Rejection::Reserved.into())
                }
                _ => push(&mut rows, text.as_bytes(), selected, host.budget())?,
            }
        }

        let mut has_manifest = false;
        for row in &rows {
            has_manifest |= row.archive.eq_ignore_ascii_case(b"Cargo.toml");
        }
        if has_manifest {
            rows.retain(|row| !row.archive.eq_ignore_ascii_case(b"Cargo.toml"));
            push(&mut rows, b"Cargo.toml.orig", input.manifest, host.budget())?;
            generate(&mut rows, b"Cargo.toml", Generated::Manifest, host.budget())?;
        }
        if input.include_lockfile {
            // Upstream lockfile.exists() affects only the optional path inside
            // GeneratedFile::Lockfile. Occupancy is unconditional in this branch.
            generate(&mut rows, b"Cargo.lock", Generated::Lockfile, host.budget())?;
        }
        if input.vcs {
            generate(
                &mut rows,
                b".cargo_vcs_info.json",
                Generated::VcsInfo,
                host.budget(),
            )?;
        }

        let mut missing_license = false;
        if let Some(license) = input.license {
            let absolute = normalize(root, native(license), host.budget())?;
            if is_file(&absolute, host)? {
                augment(root, native(license), &absolute, &mut rows, host.budget())?;
            } else {
                missing_license = true;
            }
        }
        let mut missing_readme = false;
        if let Some(readme) = input.readme {
            let absolute = normalize(root, native(readme), host.budget())?;
            if is_file(&absolute, host)? {
                augment(root, native(readme), &absolute, &mut rows, host.budget())?;
            } else {
                missing_readme = true;
            }
        }
        if missing_license || missing_readme {
            return Err(Rejection::Missing {
                license: missing_license,
                readme: missing_readme,
            }
            .into());
        }

        for script in input.build_scripts {
            let absolute = normalize(root, native(script.as_ref()), host.budget())?;
            if !is_file(&absolute, host)? || !native(&absolute).starts_with(root) {
                return Err(Rejection::BuildScript.into());
            }
        }

        host.budget()
            .sort(&mut rows, |a, b| native(&a.archive).cmp(native(&b.archive)));
        Ok(Self { rows })
    }

    pub(super) fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub(super) fn physical(&self) -> impl Iterator<Item = (&[u8], &[u8])> {
        self.rows
            .iter()
            .filter_map(|row| row.origin().map(|origin| (row.archive(), origin)))
    }
}

impl Row {
    pub(super) fn archive(&self) -> &[u8] {
        &self.archive
    }
    pub(super) fn origin(&self) -> Option<&[u8]> {
        match &self.contents {
            Contents::Copy(origin) => Some(origin),
            Contents::Generate(_) => None,
        }
    }
    pub(super) fn generated(&self) -> Option<Generated> {
        match self.contents {
            Contents::Copy(_) => None,
            Contents::Generate(kind) => Some(kind),
        }
    }
}

fn native(bytes: &[u8]) -> &path::Path {
    path::Path::new(OsStr::from_bytes(bytes))
}

fn is_file(absolute: &[u8], host: &mut host::Host<'_, '_>) -> Result<bool> {
    // Cargo's Path::is_file converts stat failures into false. Required host
    // observation errors remain errors; absent and nonfiles are false.
    Ok(host
        .metadata(absolute, true)?
        .is_some_and(|meta| meta.kind == observed::Kind::File))
}

fn filename(file: &path::Path) -> Result<()> {
    let Some(name) = file.file_name() else {
        return Ok(());
    };
    let name = name.to_str().ok_or(Rejection::Unicode)?;
    for character in ['/', '\\', '<', '>', ':', '"', '|', '?', '*'] {
        if name.contains(character) {
            return Err(Rejection::Filename(character).into());
        }
    }
    Ok(())
}

fn push(rows: &mut Vec<Row>, archive: &[u8], origin: &[u8], budget: &mut Budget<'_>) -> Result<()> {
    budget.reserve(rows, 1)?;
    let archive = budget.string(archive)?;
    let origin = budget.string(origin)?;
    rows.push(Row {
        archive,
        contents: Contents::Copy(origin),
    });
    Ok(())
}

fn generate(
    rows: &mut Vec<Row>,
    archive: &[u8],
    kind: Generated,
    budget: &mut Budget<'_>,
) -> Result<()> {
    budget.reserve(rows, 1)?;
    let archive = budget.string(archive)?;
    rows.push(Row {
        archive,
        contents: Contents::Generate(kind),
    });
    Ok(())
}

fn augment(
    root: &path::Path,
    declared: &path::Path,
    absolute: &[u8],
    rows: &mut Vec<Row>,
    budget: &mut Budget<'_>,
) -> Result<()> {
    let relative = match native(absolute).strip_prefix(root) {
        Ok(relative) => relative.as_os_str(),
        Err(_) => declared.file_name().ok_or(Rejection::Basename)?,
    };
    for row in rows.iter() {
        if native(&row.archive) == path::Path::new(relative) {
            return Ok(());
        }
    }
    let relative = relative.to_str().ok_or(Rejection::Unicode)?;
    push(rows, relative.as_bytes(), absolute, budget)
}

fn normalize(root: &path::Path, declared: &path::Path, budget: &mut Budget<'_>) -> Result<Vec<u8>> {
    let capacity = root
        .as_os_str()
        .len()
        .checked_add(declared.as_os_str().len())
        .and_then(|value| value.checked_add(1))
        .ok_or(Error::Encoding("archive path overflow"))?;
    budget.allocation.items(1)?;
    budget.allocation.bytes(capacity)?;
    let mut joined = path::PathBuf::new();
    joined.try_reserve_exact(capacity)?;
    joined.push(root);
    joined.push(declared);
    budget.allocation.items(1)?;
    budget.allocation.bytes(capacity)?;
    let mut normalized = path::PathBuf::new();
    normalized.try_reserve_exact(capacity)?;
    for component in joined.components() {
        match component {
            path::Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            path::Component::RootDir => normalized.push(component.as_os_str()),
            path::Component::CurDir => {}
            path::Component::ParentDir => {
                normalized.pop();
            }
            path::Component::Normal(value) => normalized.push(value),
        }
    }
    budget.string(normalized.as_os_str().as_bytes())
}

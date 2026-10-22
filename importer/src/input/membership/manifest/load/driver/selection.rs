//! Borrowed manifest inputs for file selection; no selected-file authority.
use super::*;
use crate::input::membership::archive;
use crate::input::membership::manifest::load::convert::build;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;

#[cfg(test)]
mod tests;

pub(in super::super) struct Package<'a> {
    pub(in super::super) manifest: &'a Path,
    pub(in super::super) root: &'a Path,
    pub(in super::super) include: &'a [String],
    pub(in super::super) exclude: &'a [String],
    /// Normalized package-relative spelling, not the SPDX license expression.
    pub(in super::super) license_file: Option<&'a str>,
    pub(in super::super) readme: Option<&'a str>,
    builds: &'a [build::Target],
}
impl<'inputs> Package<'inputs> {
    /// Execute the explicit-include branch against actual traversal evidence.
    /// Packages without includes still need the separate Git-discovery join.
    pub(in super::super) fn included<'package>(
        &'package self,
        session: &mut Session<'_, host::Host<'_, '_>>,
    ) -> Step<
        Result<
            Option<super::super::selection::Files<'package, 'inputs>>,
            super::super::selection::Failure,
        >,
    > {
        super::super::selection::included(self, session)
    }

    /// Selection and command/VCS facts remain explicit caller inputs. The
    /// archive projector resolves augmentation paths only when Cargo does.
    pub(in super::super) fn project(
        &self,
        selected: &[&[u8]],
        include_lockfile: bool,
        vcs: bool,
        session: &mut Session<'_, host::Host<'_, '_>>,
    ) -> Step<archive::Projection> {
        session.archive(archive::Inputs {
            root: self.root.as_os_str().as_bytes(),
            manifest: self.manifest.as_os_str().as_bytes(),
            selected,
            license: self.license_file.map(str::as_bytes),
            readme: self.readme.map(str::as_bytes),
            build_scripts: self.build_scripts().map(|path| path.as_os_str().as_bytes()),
            include_lockfile,
            vcs,
        })
    }
    pub(in super::super) fn build_scripts(&self) -> impl Iterator<Item = &Path> {
        self.builds
            .iter()
            .filter_map(|target| match target.source() {
                build::Source::Path(path) => Some(path.as_path()),
                build::Source::Metabuild => None,
            })
    }
}
impl convert::Finished<'_, '_, '_, '_, '_> {
    /// Only fully converted, report-attached real manifests reach selection.
    /// Virtual manifests have no package; pending and failed reports cannot
    /// call this method. All fields borrow existing storage without cloning.
    pub(in super::super) fn package<A: rawr_toml::document::Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<Option<Package<'_>>> {
        session.check()?;
        let Converted::Real(real) = &self.output else {
            return Ok(None);
        };
        let Some(root) = self.owner.manifest().parent() else {
            return session.fail(Error::State);
        };
        let Some(contents) = real.dependencies.contents.as_ref() else {
            return session.fail(Error::State);
        };
        let Some(metadata) = real.dependencies.prepared.metadata.as_ref() else {
            return session.fail(Error::State);
        };
        Ok(Some(Package {
            manifest: self.owner.manifest(),
            root,
            include: contents.include.as_slice(),
            exclude: contents.exclude.as_slice(),
            license_file: metadata.license_file.as_deref(),
            readme: metadata.readme.as_deref(),
            builds: real.targets.builds.as_slice(),
        }))
    }
}

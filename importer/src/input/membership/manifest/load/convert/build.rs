// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../path/LICENSE-MIT and ../path/LICENSE-APACHE.
//! Custom-build and metabuild tail of to_targets, targets.rs:101–141.

mod stems;
use super::super::super::{session::Session, storage, value, Error, Step};
use super::super::{
    normalize::{dependencies, files, target},
    path,
};
use super::{edition, target::configure};
use rawr_toml::document;
use std::path::{Path, PathBuf};

pub(in super::super) enum Source {
    Path(PathBuf),
    Metabuild,
}
pub(in super::super) struct Target {
    name: String,
    source: Source,
    settings: configure::Settings,
}
impl Target {
    pub(in super::super) fn name(&self) -> &str {
        &self.name
    }
    pub(in super::super) fn source(&self) -> &Source {
        &self.source
    }
    pub(in super::super) fn settings(&self) -> &configure::Settings {
        &self.settings
    }
}

pub(in super::super) fn convert<A: document::Admission>(
    build: &files::Build<'_, '_>,
    metabuild: Option<&[value::Text<'_>]>,
    dependencies: Option<&[dependencies::Dependency<'_, '_>]>,
    package_name: &str,
    root: &Path,
    edition: edition::Edition,
    session: &mut Session<'_, A>,
) -> Step<Result<storage::Buffer<Target>, String>> {
    session.check()?;
    let has_build = match build {
        files::Build::Auto(false) => false,
        files::Build::Auto(true) => return session.fail(Error::State),
        _ => true,
    };
    let mut output = storage::Buffer::new();
    if has_build {
        if metabuild.is_some() {
            return target::formatted(
                format_args!("cannot specify both `metabuild` and `build`"),
                session,
            )
            .map(Err);
        }
        let mut scripts = storage::Buffer::new();
        match build {
            files::Build::Single(text) => {
                scripts.push(text.as_str(), session)?;
            }
            files::Build::Multiple(texts) => {
                for text in *texts {
                    scripts.push(text.as_str(), session)?;
                }
            }
            _ => return session.fail(Error::State),
        }
        if let Err(error) = stems::check(scripts.as_slice(), session)? {
            return Ok(Err(error));
        }
        for script in scripts.as_slice() {
            let script = Path::new(script);
            let stem = script
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("");
            let name = target::formatted(format_args!("build-script-{stem}"), session)?;
            let source = Source::Path(path::join(root, script, session)?);
            let settings = configure::defaults(configure::Kind::CustomBuild, edition, session)?;
            output.push(
                Target {
                    name,
                    source,
                    settings,
                },
                session,
            )?;
        }
    }
    if let Some(metabuild) = metabuild {
        for name in metabuild {
            session.items(1)?;
            let found = dependencies.is_some_and(|dependencies| {
                dependencies
                    .binary_search_by(|dependency| dependency.name().cmp(name.as_str()))
                    .is_ok()
            });
            if !found {
                return target::formatted(
                    format_args!(
                        "metabuild package `{}` must be specified in `build-dependencies`",
                        name.as_str()
                    ),
                    session,
                )
                .map(Err);
            }
        }
        let name = target::formatted(format_args!("metabuild-{package_name}"), session)?;
        let settings = configure::defaults(configure::Kind::Metabuild, edition, session)?;
        output.push(
            Target {
                name,
                source: Source::Metabuild,
                settings,
            },
            session,
        )?;
    }
    Ok(Ok(output))
}

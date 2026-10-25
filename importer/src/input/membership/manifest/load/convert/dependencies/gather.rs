// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../../path/LICENSE-MIT and ../../path/LICENSE-APACHE.
//! Source-ordered dependency collection before replace/patch and cross-source checks.
use super::super::{dependency, platform, source};
use crate::cargo;
use crate::input::membership::{
    host,
    manifest::{
        load::{
            config,
            normalize::{dependencies::Kind, tail},
            Warnings,
        },
        session::Session,
        storage, Step,
    },
};
use std::path::Path;

// A continuation step that is matched at once; boxing would allocate per step.
#[allow(clippy::large_enum_variant)]
pub(in super::super::super) enum Progress<'a, 'd, 's> {
    Complete(Collected<'a, 's>),
    Canonical(Pending<'a, 'd, 's>),
}

/// Cargo carries the last parsed platform into replace/patch conversion, even
/// when that platform contains no dependencies.
pub(in super::super::super) struct Collected<'a, 's> {
    completed: storage::Buffer<dependency::Converted<'a, 's>>,
    platform: Option<platform::Platform<'a>>,
}
impl<'a, 's> Collected<'a, 's> {
    pub(in super::super::super) fn as_slice(&self) -> &[dependency::Converted<'a, 's>] {
        self.completed.as_slice()
    }
    pub(in super::super::super) fn platform(&self) -> Option<&platform::Platform<'a>> {
        self.platform.as_ref()
    }
}

/// Collection and exact cursor survive the pending declaration. No arbitrary
/// completion or partial-list extraction can grant a complete manifest.
pub(in super::super::super) struct Pending<'a, 'd, 's> {
    cursor: Cursor<'a, 'd, 's>,
    dependency: dependency::Pending<'a, 'a, 's>,
}
impl Pending<'_, '_, '_> {
    pub(in super::super::super) fn spelling(&self) -> &str {
        self.dependency.spelling()
    }
    pub(in super::super::super) fn dependency_name(&self) -> &str {
        self.dependency.dependency_name()
    }
    pub(in super::super::super) fn fields(&self) -> &dependency::record::BeforeArtifact<'_, '_> {
        self.dependency.fields()
    }
    #[cfg(test)]
    pub(in super::super::super) fn completed_count(&self) -> usize {
        self.cursor.completed.as_slice().len()
    }
}

#[derive(Clone, Copy)]
enum Stage {
    Top(usize),
    Platform(usize, usize),
    Done,
}
struct Cursor<'a, 'd, 's> {
    tail: &'a tail::Complete<'a, 'd>,
    stage: Stage,
    index: usize,
    platform: Option<platform::Platform<'a>>,
    completed: storage::Buffer<dependency::Converted<'a, 's>>,
}

#[allow(clippy::too_many_arguments)]
pub(in super::super::super) fn convert<'a, 'd: 'a, 's, 'image: 's>(
    tail: &'a tail::Complete<'a, 'd>,
    manifest: &Path,
    parent: source::Id<'s>,
    sources: &mut source::Owner<'s>,
    config: &mut config::Owner<'s, 'image>,
    bindeps: bool,
    json_target_spec: bool,
    limits: &cargo::Limits,
    warnings: &mut Warnings,
    session: &mut Session<'_, host::Host<'image, '_>>,
) -> Step<Result<Progress<'a, 'd, 's>, String>> {
    session.check()?;
    let mut cursor = Cursor {
        tail,
        stage: Stage::Top(0),
        index: 0,
        platform: None,
        completed: storage::Buffer::new(),
    };
    loop {
        session.items(1)?;
        let (map, kind) = match cursor.stage {
            Stage::Top(group) if group < 3 => {
                let kind = [Kind::Normal, Kind::Development, Kind::Build][group];
                (cursor.tail.maps(), kind)
            }
            Stage::Top(_) => {
                cursor.stage = Stage::Platform(0, 0);
                continue;
            }
            Stage::Platform(index, group) => {
                let Some(entry) = cursor
                    .tail
                    .platforms()
                    .and_then(|entries| entries.get(index))
                else {
                    cursor.stage = Stage::Done;
                    continue;
                };
                if group == 3 {
                    cursor.stage = Stage::Platform(index + 1, 0);
                    continue;
                }
                if group == 0 && cursor.index == 0 {
                    cursor.platform = Some(match platform::parse(entry.name(), session)? {
                        Ok(platform) => platform,
                        Err(error) => return Ok(Err(error)),
                    });
                }
                let kind = [Kind::Normal, Kind::Build, Kind::Development][group];
                (entry.maps(), kind)
            }
            Stage::Done => {
                return Ok(Ok(Progress::Complete(Collected {
                    completed: cursor.completed,
                    platform: cursor.platform,
                })))
            }
        };
        let Some(declaration) = map.get(kind).and_then(|entries| entries.get(cursor.index)) else {
            cursor.index = 0;
            cursor.stage = match cursor.stage {
                Stage::Top(group) => Stage::Top(group + 1),
                Stage::Platform(index, group) => Stage::Platform(index, group + 1),
                Stage::Done => {
                    return session.fail(crate::input::membership::manifest::Error::State)
                }
            };
            continue;
        };
        let converted = match dependency::convert(
            declaration,
            manifest,
            parent,
            sources,
            config,
            kind,
            cursor.platform.as_ref(),
            bindeps,
            json_target_spec,
            limits,
            warnings,
            session,
        )? {
            Ok(converted) => converted,
            Err(error) => return Ok(Err(error)),
        };
        cursor.index += 1;
        match converted {
            dependency::Progress::Complete(value) => {
                cursor.completed.push(value, session)?;
            }
            dependency::Progress::Canonical(dependency) => {
                return Ok(Ok(Progress::Canonical(Pending { cursor, dependency })))
            }
        }
    }
}

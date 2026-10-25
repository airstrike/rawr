// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../../path/LICENSE-MIT and ../../path/LICENSE-APACHE.
//! Patch groups: complete values precede URL-keyed replacement.
use super::super::{platform, source};
use super::{entry, registry};
use crate::{
    cargo,
    input::membership::{
        host,
        manifest::{
            load::{config, normalize, Warnings},
            session::Session,
            spec::url::Url,
            storage, Step,
        },
    },
};
use std::path::Path;

pub(in super::super::super) struct Group<'p, 's> {
    url: Url,
    entries: storage::Buffer<entry::Entry<'p, 's>>,
}
impl<'p, 's> Group<'p, 's> {
    pub(in super::super::super) fn url(&self) -> &Url {
        &self.url
    }
    pub(in super::super::super) fn entries(&self) -> &[entry::Entry<'p, 's>] {
        self.entries.as_slice()
    }
}
pub(in super::super::super) struct Map<'p, 's>(storage::Buffer<Group<'p, 's>>);
impl<'p, 's> Map<'p, 's> {
    /// Sorted transformed storage, not an assertion of Cargo HashMap iteration order.
    pub(in super::super::super) fn groups(&self) -> &[Group<'p, 's>] {
        self.0.as_slice()
    }
    fn insert<A: rawr_toml::document::Admission>(
        &mut self,
        group: Group<'p, 's>,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        let mut low = 0;
        let mut high = self.0.as_slice().len();
        while low < high {
            session.items(1)?;
            let middle = low + (high - low) / 2;
            let left = self.0.as_slice()[middle].url.as_str(session)?;
            let right = group.url.as_str(session)?;
            session.items(left.len().min(right.len()))?;
            match left.cmp(right) {
                std::cmp::Ordering::Less => low = middle + 1,
                std::cmp::Ordering::Greater => high = middle,
                std::cmp::Ordering::Equal => {
                    // HashMap::insert replaces the value, retaining the first key.
                    self.0.as_mut_slice()[middle].entries = group.entries;
                    return Ok(());
                }
            }
        }
        let length = self.0.as_slice().len();
        session.items(length)?;
        session.items(length - low)?;
        self.0.push(group, session)?;
        self.0.as_mut_slice()[low..].rotate_right(1);
        Ok(())
    }
}
// A continuation step that is matched at once; boxing would allocate per step.
#[allow(clippy::large_enum_variant)]
pub(in super::super::super) enum Progress<'a, 'p, 's> {
    Complete(Map<'p, 's>),
    Canonical(Pending<'a, 'p, 's>),
}
pub(in super::super::super) struct Pending<'a, 'p, 's> {
    entry: entry::Pending<'a, 'p, 's>,
}
impl Pending<'_, '_, '_> {
    pub(in super::super::super) fn spelling(&self) -> &str {
        self.entry.spelling()
    }
}

/// Called after replacement handling. This does not grant full manifest authority.
#[allow(clippy::too_many_arguments)]
pub(in super::super::super) fn convert<'a, 'd: 'a, 'p, 's, 'image: 's>(
    patches: Option<&'a [normalize::Source<'a, 'd>]>,
    manifest: &'a Path,
    parent: source::Id<'s>,
    sources: &mut source::Owner<'s>,
    config: &mut config::Owner<'s, 'image>,
    platform: Option<&platform::Platform<'p>>,
    bindeps: bool,
    json_target_spec: bool,
    limits: &cargo::Limits,
    warnings: &mut Warnings,
    session: &mut Session<'_, host::Host<'image, '_>>,
) -> Step<Result<Progress<'a, 'p, 's>, String>> {
    session.check()?;
    let patches = patches.unwrap_or(&[]);
    let mut completed = Map(storage::Buffer::new());
    for patch in patches {
        session.items(1)?;
        let url = match registry(patch.name, config, limits, session)? {
            Ok(url) => url,
            Err(error) => return Ok(Err(error)),
        };
        let mut active = Group {
            url,
            entries: storage::Buffer::new(),
        };
        for declaration in patch.dependencies() {
            session.items(1)?;
            let converted = match entry::convert(
                declaration,
                patch.name,
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
                Ok(value) => value,
                Err(error) => return Ok(Err(error)),
            };
            match converted {
                entry::Progress::Complete(value) => {
                    active.entries.push(value, session)?;
                }
                entry::Progress::Canonical(entry) => {
                    return Ok(Ok(Progress::Canonical(Pending { entry })))
                }
            }
        }
        completed.insert(active, session)?;
    }
    Ok(Ok(Progress::Complete(completed)))
}

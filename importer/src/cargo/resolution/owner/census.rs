//! Development byte census; never used to construct an owner or admit content.

use std::fs;
use std::path::Path;

use super::super::content::Scope;
use crate::extract::plan::Capture;
use crate::node::{Error, GraphBudget};
use crate::store::{self, bundle};

#[cfg(test)]
mod tests;

#[derive(Default)]
struct Inventory {
    members: usize,
    directories: usize,
    relative_path_bytes: usize,
    logical_file_bytes: u64,
    directory_metadata_bytes: u64,
    allocated_blocks: u64,
    conservative_files: usize,
    conservative_bytes: u64,
    roots: u8,
}

#[derive(Default)]
struct Traversal {
    files: usize,
    directories: usize,
    all: u64,
    broad: u64,
    selected: u64,
    selected_files: usize,
    selected_parent_components: usize,
    inventory: Inventory,
}

/// Development attribution only; cumulative counters retain their original account.
pub(super) fn checkpoint(stage: &str, graph: &GraphBudget) -> Result<(), Error> {
    if option_env!("RAWR_SCOPE_CENSUS") != Some("1") {
        return Ok(());
    }
    use std::io::Write as _;
    let (decoded, paths, hashed) = graph.census_usage()?;
    let costs = graph.census_paths()?;
    let mut output = std::io::stderr().lock();
    writeln!(
        output,
        "rawr-path-stage-v1\t{stage}\t{decoded}\t{paths}\t{hashed}"
    )
    .map_err(|_| Error::Codec("path census output failed"))?;
    for row in costs.rows.iter().flatten() {
        writeln!(
            output,
            "rawr-path-cost-v1\t{stage}\t{}\t{}\t{}\t{}",
            row.file, row.line, row.counts.admitted, row.counts.rejected
        )
        .map_err(|_| Error::Codec("path census output failed"))?;
    }
    writeln!(
        output,
        "rawr-path-overflow-v1\t{stage}\t{}\t{}\t{}\t{}",
        costs.enabled, costs.overflow.admitted, costs.overflow.rejected, costs.overflow.saturated
    )
    .map_err(|_| Error::Codec("path census output failed"))
}

pub(super) fn report(
    root: &Path,
    conservative: &Scope,
    query: &Scope,
    capture: &Capture,
    graph: &GraphBudget,
) -> Result<(), Error> {
    checkpoint("before-inventory-census", graph)?;
    let before = graph.census_usage()?;
    let Traversal {
        files,
        directories,
        all,
        broad,
        selected,
        selected_files,
        selected_parent_components,
        inventory,
    } = traverse(root, conservative, Some(query), graph)?;
    let inventory_members = inventory.members;
    let inventory_directories = inventory.directories;
    let namespace = capture
        .namespace()
        .ok_or(Error::Codec("scope census namespace absent"))?;
    let (mut prehash, mut local_selected, mut external_selected) = (0u64, 0u64, 0u64);
    let (mut local_selected_files, mut external_selected_files) = (0usize, 0usize);
    let (mut local_members, mut local_directories, mut local_query_members) =
        (0usize, 0usize, 0usize);
    for member in &namespace.members {
        let in_inventory = matches!(
            member.path.strip_prefix(root).ok().and_then(|path| path.components().next()),
            Some(std::path::Component::Normal(name))
                if name == "build" || name == "declared" || name == "generated" || name == "target"
        );
        if in_inventory {
            local_members += 1;
            local_directories += usize::from(member.facts.kind == 0);
            local_query_members +=
                usize::from(member.facts.kind == 1 && query.requires_content(&member.path));
        }
        if member.candidate {
            prehash = prehash
                .checked_add(member.facts.length)
                .ok_or(Error::Allocation)?;
        }
        if member.selected {
            let count = if member.path.starts_with(root) {
                local_selected_files += 1;
                &mut local_selected
            } else {
                external_selected_files += 1;
                &mut external_selected
            };
            *count = count
                .checked_add(member.facts.length)
                .ok_or(Error::Allocation)?;
        }
    }
    let after = graph.census_usage()?;
    eprintln!("rawr-namespace-inventory-overlap-v1\tunique_members={}\tlocal_members={local_members}\tlocal_directories={local_directories}\tlocal_query_members={local_query_members}\texternal_members={}", namespace.members.len(), namespace.members.len() - local_members);
    eprintln!("rawr-scope-census-v1\tfiles={files}\tdirectories={directories}\tall_local_bytes={all}\tconservative_local_bytes={broad}\tquery_independent_local_bytes={selected}\tnamespace_prehash_bytes={prehash}\tnamespace_local_selected_bytes={local_selected}\tnamespace_external_selected_bytes={external_selected}\tbefore_decode={}\tbefore_paths={}\tbefore_hash={}\tafter_decode={}\tafter_paths={}\tafter_hash={}", before.0,before.1,before.2,after.0,after.1,after.2);
    eprintln!("rawr-scope-path-census-v1\tinventory_members={inventory_members}\tinventory_directories={inventory_directories}\tquery_independent_files={selected_files}\tselected_parent_components={selected_parent_components}\tnamespace_local_selected_files={local_selected_files}\tnamespace_external_selected_files={external_selected_files}");
    Ok(())
}

impl Inventory {
    fn observe(
        &mut self,
        relative: &Path,
        metadata: &fs::Metadata,
        conservative: bool,
    ) -> Result<(), Error> {
        let Some(root) = inventory_root(relative) else {
            return Ok(());
        };
        self.members = self.members.checked_add(1).ok_or(Error::Allocation)?;
        self.relative_path_bytes = self
            .relative_path_bytes
            .checked_add(relative.as_os_str().as_encoded_bytes().len())
            .ok_or(Error::Allocation)?;
        self.allocated_blocks = self
            .allocated_blocks
            .checked_add(allocated_blocks(metadata)?)
            .ok_or(Error::Allocation)?;
        if metadata.is_dir() {
            self.directories = self.directories.checked_add(1).ok_or(Error::Allocation)?;
            self.directory_metadata_bytes = self
                .directory_metadata_bytes
                .checked_add(metadata.len())
                .ok_or(Error::Allocation)?;
            if relative.components().count() == 1 {
                self.roots |= root;
            }
        } else if metadata.is_file() {
            self.logical_file_bytes = self
                .logical_file_bytes
                .checked_add(metadata.len())
                .ok_or(Error::Allocation)?;
            if conservative {
                self.conservative_files = self
                    .conservative_files
                    .checked_add(1)
                    .ok_or(Error::Allocation)?;
                self.conservative_bytes = self
                    .conservative_bytes
                    .checked_add(metadata.len())
                    .ok_or(Error::Allocation)?;
            }
        }
        within_bundle_bytes(self.logical_file_bytes)?;
        within_bundle_bytes(self.allocated_bytes()?)
    }

    fn allocated_bytes(&self) -> Result<u64, Error> {
        self.allocated_blocks
            .checked_mul(512)
            .ok_or(Error::Allocation)
    }
}

/// Extend the existing metadata traversal; each member is observed only once.
fn traverse(
    root: &Path,
    conservative: &Scope,
    query: Option<&Scope>,
    graph: &GraphBudget,
) -> Result<Traversal, Error> {
    let mut observed = Traversal::default();
    let mut pending = bundle::buffer::Buffer::new();
    pending.push(bundle::path(root, Path::new(""), graph)?, graph)?;
    while let Some(directory) = pending.pop() {
        observed.directories = observed
            .directories
            .checked_add(1)
            .ok_or(Error::Allocation)?;
        let names = bundle::names(&directory, bundle::MAX_MEMBERS as usize, graph)?;
        for name in names {
            let members = observed
                .files
                .checked_add(observed.directories)
                .and_then(|count| count.checked_add(pending.len()))
                .and_then(|count| count.checked_add(1))
                .ok_or(Error::Allocation)?;
            if members > bundle::MAX_MEMBERS as usize {
                return Err(Error::Limit {
                    resource: "scope census members",
                    limit: bundle::MAX_MEMBERS as usize,
                    actual: members,
                });
            }
            let path = bundle::path(&directory, Path::new(&name), graph)?;
            graph.paths(1)?;
            store::path::input(&path, graph)?;
            let metadata = fs::symlink_metadata(&path).map_err(|source| Error::Io {
                operation: "scope census metadata",
                path: path.clone(),
                source,
            })?;
            if metadata.file_type().is_symlink()
                || bundle::hard_linked(&metadata) && metadata.is_file()
            {
                return Err(Error::Codec("scope census unsupported link"));
            }
            let relative = path
                .strip_prefix(root)
                .map_err(|_| Error::Codec("scope census member escaped allocation"))?;
            let selected = conservative.requires_content(&path);
            observed.inventory.observe(relative, &metadata, selected)?;
            if metadata.is_dir() {
                pending.push(path, graph)?;
            } else if metadata.is_file() {
                observed.files = observed.files.checked_add(1).ok_or(Error::Allocation)?;
                observed.all = observed
                    .all
                    .checked_add(metadata.len())
                    .ok_or(Error::Allocation)?;
                within_bundle_bytes(observed.all)?;
                if selected {
                    observed.broad = observed
                        .broad
                        .checked_add(metadata.len())
                        .ok_or(Error::Allocation)?;
                }
                if query.is_some_and(|query| query.requires_content(&path)) {
                    observed.selected = observed
                        .selected
                        .checked_add(metadata.len())
                        .ok_or(Error::Allocation)?;
                    if inventory_root(relative).is_some() {
                        observed.selected_files = observed
                            .selected_files
                            .checked_add(1)
                            .ok_or(Error::Allocation)?;
                        let parent = path.parent().ok_or(Error::Codec("census parent absent"))?;
                        observed.selected_parent_components = observed
                            .selected_parent_components
                            .checked_add(
                                parent
                                    .components()
                                    .filter(|component| {
                                        matches!(component, std::path::Component::Normal(_))
                                    })
                                    .count(),
                            )
                            .ok_or(Error::Allocation)?;
                    }
                }
            } else {
                return Err(Error::Codec("scope census unsupported type"));
            }
        }
    }
    Ok(observed)
}

fn inventory_root(relative: &Path) -> Option<u8> {
    match relative.components().next()? {
        std::path::Component::Normal(name) if name == "build" => Some(1),
        std::path::Component::Normal(name) if name == "declared" => Some(2),
        std::path::Component::Normal(name) if name == "generated" => Some(4),
        std::path::Component::Normal(name) if name == "target" => Some(8),
        _ => None,
    }
}

fn within_bundle_bytes(bytes: u64) -> Result<(), Error> {
    if bytes > bundle::MAX_BYTES {
        return Err(Error::Limit {
            resource: "cold producer observed bundle bytes",
            limit: usize::try_from(bundle::MAX_BYTES).map_err(|_| Error::Allocation)?,
            actual: usize::try_from(bytes).map_err(|_| Error::Allocation)?,
        });
    }
    Ok(())
}

#[cfg(unix)]
fn allocated_blocks(metadata: &fs::Metadata) -> Result<u64, Error> {
    use std::os::unix::fs::MetadataExt as _;
    Ok(metadata.blocks())
}

#[cfg(not(unix))]
fn allocated_blocks(_: &fs::Metadata) -> Result<u64, Error> {
    Err(Error::Codec("cold producer allocated blocks unavailable"))
}

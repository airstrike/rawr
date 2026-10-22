use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{traverse, within_bundle_bytes, Inventory, Scope};
use crate::node::{Error, GraphBudget};
use crate::store::bundle;

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Fixture(PathBuf);

impl Fixture {
    fn capture() -> Self {
        let root = std::env::temp_dir().join(format!(
            "rawr-cold-producer-scalars-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        for name in ["build", "declared", "generated", "target"] {
            fs::create_dir(root.join(name)).unwrap();
        }
        Self(fs::canonicalize(root).unwrap())
    }

    fn root(&self) -> &Path {
        &self.0
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn four_root_geometry_reuses_metadata_without_hashing_file_contents() {
    let fixture = Fixture::capture();
    let root = fixture.root();
    fs::create_dir(root.join("build/sub")).unwrap();
    fs::write(root.join("build/sub/file"), [1, 2, 3]).unwrap();
    fs::write(root.join("target/output"), [4, 5, 6, 7, 8]).unwrap();
    fs::write(root.join("outside-control"), [9, 10]).unwrap();
    let graph = GraphBudget::new();
    let conservative = Scope::complete(root, &graph).unwrap();
    let before = graph.census_vector().unwrap();
    let observed = traverse(root, &conservative, None, &graph).unwrap();
    let inventory = observed.inventory;
    assert_eq!(inventory.members, 7);
    assert_eq!(inventory.directories, 5);
    assert_eq!(inventory.logical_file_bytes, 8);
    assert_eq!(inventory.conservative_files, 2);
    assert_eq!(inventory.conservative_bytes, 8);
    let relatives = [
        "build",
        "declared",
        "generated",
        "target",
        "build/sub",
        "build/sub/file",
        "target/output",
    ];
    assert_eq!(
        inventory.relative_path_bytes,
        relatives.iter().map(|path| path.len()).sum::<usize>()
    );
    let expected_blocks = relatives
        .iter()
        .map(|path| {
            super::allocated_blocks(&fs::symlink_metadata(root.join(path)).unwrap()).unwrap()
        })
        .sum::<u64>();
    assert_eq!(inventory.allocated_blocks, expected_blocks);
    assert_eq!(inventory.allocated_bytes().unwrap(), expected_blocks * 512);
    assert_eq!(observed.files, 3);
    assert_eq!(observed.directories, 6);
    assert_eq!(observed.all, 10);
    assert_eq!(observed.broad, 10);
    assert_eq!(observed.selected, 0);
    assert_eq!(observed.selected_files, 0);
    let after = graph.census_vector().unwrap();
    assert!(after[3] > before[3]);
    assert!(after[4] > before[4]);
    assert_eq!(
        after[5], before[5],
        "metadata-only geometry must not hash contents"
    );
}

#[test]
fn census_records_the_original_exhausted_graph_without_resetting_it() {
    let fixture = Fixture::capture();
    let graph = GraphBudget::new();
    let conservative = Scope::complete(fixture.root(), &graph).unwrap();
    let paths = graph.census_vector().unwrap()[4] as usize;
    graph
        .paths(crate::node::MAX_PATH_CHECKS_PER_GRAPH - paths)
        .unwrap();
    // Exactly at the per-graph path target admits silently.
    assert!(!graph.exceedances().is_exceeded());
    let before = graph.census_vector().unwrap();
    // The census continues on the original graph and records the
    // crossed path target there instead of refusing.
    traverse(fixture.root(), &conservative, None, &graph)
        .expect("an accounting target never refuses the census");
    let after = graph.census_vector().unwrap();
    assert!(after[4] > before[4]);
    assert!(after[3] >= before[3]);
    let record = graph.exceedances();
    let axis = record
        .axis(crate::node::PATHS_RESOURCE)
        .expect("the crossed path target is recorded on the original graph");
    assert_eq!(axis.target, crate::node::MAX_PATH_CHECKS_PER_GRAPH as u64);
    assert!(axis.first_requested > axis.target);
    assert_eq!(axis.high_water, after[4]);
}

#[test]
fn geometry_and_cumulative_delta_refuse_overflow_or_account_replacement() {
    within_bundle_bytes(bundle::MAX_BYTES).unwrap();
    assert!(matches!(
        within_bundle_bytes(bundle::MAX_BYTES + 1),
        Err(Error::Limit { .. })
    ));
    let inventory = Inventory {
        allocated_blocks: u64::MAX,
        ..Inventory::default()
    };
    assert!(matches!(
        inventory.allocated_bytes(),
        Err(Error::Allocation)
    ));
}

use super::*;
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT: AtomicU64 = AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rawr-workspace-input-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(fs::canonicalize(path).unwrap())
    }
    fn file(&self, relative: &str, bytes: &str) -> PathBuf {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, bytes).unwrap();
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn image(roots: &[PathBuf]) -> Image {
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = Allocation::for_replay(&limits);
    capture(
        roots.to_vec().into_boxed_slice(),
        &mut Account::borrow(&mut allocation, &graph, &limits),
    )
    .unwrap()
}

#[test]
fn external_inheritance_and_missing_ancestor_are_current_inputs() {
    let fixture = Fixture::new();
    let owner = fixture.file(
        "workspace/member/deep/Cargo.toml",
        "[package]\nname='member'\nversion.workspace=true\n",
    );
    let root = fixture.file(
        "workspace/Cargo.toml",
        "[workspace.package]\nversion='1.0.0'\n",
    );
    let before = image(std::slice::from_ref(&owner));
    fs::write(&root, "[workspace.package]\nversion='2.0.0'\n").unwrap();
    assert!(before.entry != image(std::slice::from_ref(&owner)).entry);
    let before = image(std::slice::from_ref(&owner));
    fixture.file("workspace/member/Cargo.toml", "[workspace]\n");
    assert!(before.entry != image(std::slice::from_ref(&owner)).entry);
}

#[test]
fn package_project_and_ancestor_redirects_are_closed_recursively() {
    let fixture = Fixture::new();
    let owner = fixture.file(
        "source/member/Cargo.toml",
        "[project]\nworkspace='../../first'\n",
    );
    fixture.file("first/Cargo.toml", "[package]\nworkspace='../second'\n");
    let target = fixture.file("second/Cargo.toml", "[workspace.package]\nedition='2021'\n");
    let before = image(std::slice::from_ref(&owner));
    fs::write(target, "[workspace.package]\nedition='2024'\n").unwrap();
    assert!(before.entry != image(std::slice::from_ref(&owner)).entry);
    fixture.file("source/Cargo.toml", "[package]\nworkspace='../elsewhere'\n");
    let elsewhere = fixture.file("elsewhere/Cargo.toml", "[workspace]\n");
    let before = image(std::slice::from_ref(&owner));
    fs::write(elsewhere, "[workspace]\nresolver='3'\n").unwrap();
    assert!(before.entry != image(std::slice::from_ref(&owner)).entry);
}

#[test]
fn unrelated_malformed_and_directory_candidates_are_observations() {
    let fixture = Fixture::new();
    let owner = fixture.file(
        "unrelated/member/Cargo.toml",
        "[package]\nname='member'\nversion='1.0.0'\n",
    );
    let ancestor = fixture.file("unrelated/Cargo.toml", "[[ not a document");
    let before = image(std::slice::from_ref(&owner));
    fs::remove_file(&ancestor).unwrap();
    fs::create_dir(&ancestor).unwrap();
    fixture.file(
        "unrelated/Cargo.toml/never-read",
        "ignored directory content",
    );
    let directory = image(std::slice::from_ref(&owner));
    assert!(before.entry != directory.entry);
    fixture.file("unrelated/Cargo.toml/never-read", "changed ignored content");
    assert!(directory.entry == image(std::slice::from_ref(&owner)).entry);
}

#[test]
fn equal_content_symlink_retarget_changes_canonical_association() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let owner = fixture.file("member/Cargo.toml", "[package]\nworkspace='../selected'\n");
    fixture.file("first/Cargo.toml", "[workspace]\n");
    fixture.file("second/Cargo.toml", "[workspace]\n");
    let selected = fixture.0.join("selected");
    symlink(fixture.0.join("first"), &selected).unwrap();
    let before = image(std::slice::from_ref(&owner));
    fs::remove_file(&selected).unwrap();
    symlink(fixture.0.join("second"), &selected).unwrap();
    assert!(before.entry != image(std::slice::from_ref(&owner)).entry);
}

#[test]
fn repeated_workspace_observations_keep_original_graph_and_ownership() {
    let fixture = Fixture::new();
    let mut source = String::from("[package]\nname='member'\nversion='1'\n[dependencies]\n");
    for index in 0..100 {
        source.push_str(&format!("dependency{index}='1'\n"));
    }
    let owner = fixture.file("member/Cargo.toml", &source);
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = Allocation::for_replay(&limits);
    let before = capture(
        Box::from([owner]),
        &mut Account::borrow(&mut allocation, &graph, &limits),
    )
    .unwrap();
    let spent = allocation.usage();
    let original_graph = graph.census_usage().unwrap();
    before
        .require(&mut Account::borrow(&mut allocation, &graph, &limits))
        .unwrap();
    assert!(allocation.usage().items > spent.items);
    assert!(allocation.usage().bytes > spent.bytes);
    let checked = allocation.usage();
    let checked_graph = graph.census_usage().unwrap();
    assert!(
        checked.bytes - spent.bytes < spent.bytes,
        "the live complete before-image does not repeat TOML derivation ownership"
    );
    assert!(
        checked_graph.1 > original_graph.1,
        "actual reads and shape/canonical checks spend the original graph"
    );
    assert_eq!(
        checked_graph.2 - original_graph.2,
        original_graph.2,
        "every original framed byte is hashed again"
    );
    before
        .require(&mut Account::borrow(&mut allocation, &graph, &limits))
        .unwrap();
    let repeated = allocation.usage();
    let repeated_graph = graph.census_usage().unwrap();
    assert_eq!(repeated.items - checked.items, checked.items - spent.items);
    assert_eq!(repeated.bytes - checked.bytes, checked.bytes - spent.bytes);
    assert_eq!(
        repeated_graph.1 - checked_graph.1,
        checked_graph.1 - original_graph.1
    );
    assert_eq!(
        repeated_graph.2 - checked_graph.2,
        checked_graph.2 - original_graph.2
    );
    // A graph whose hashing target is already spent still completes the
    // comparison; the hashed axis records exactly the bytes one require hashes.
    let exhausted = node::GraphBudget::new();
    exhausted
        .hash(node::MAX_HASHED_BYTES_PER_GRAPH as usize)
        .unwrap();
    assert!(!exhausted.exceedances().is_exceeded());
    before
        .require(&mut Account::borrow(&mut allocation, &exhausted, &limits))
        .expect("an exhausted hashing target records an exceedance, never a refusal");
    let hashed = exhausted
        .exceedances()
        .axis(node::HASHED_RESOURCE)
        .copied()
        .expect("the hashed axis is exceeded");
    assert_eq!(hashed.target, node::MAX_HASHED_BYTES_PER_GRAPH);
    assert_eq!(
        hashed.high_water,
        node::MAX_HASHED_BYTES_PER_GRAPH + (checked_graph.2 - original_graph.2)
    );
    assert_eq!(exhausted.census_usage().unwrap().2, hashed.high_water);
    allocation
        .items(0)
        .expect("a graph exceedance never latches the input account");
}

#[test]
fn live_workspace_before_image_rejects_redirect_symlink_and_ancestor_changes() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let source = "[package]\nworkspace='../selected'\n";
    let owner = fixture.file("member/Cargo.toml", source);
    let first = fixture.file("first/Cargo.toml", "[workspace]\n");
    fixture.file("second/Cargo.toml", "[workspace]\n");
    let selected = fixture.0.join("selected");
    symlink(fixture.0.join("first"), &selected).unwrap();
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = Allocation::for_replay(&limits);
    let mut account = Account::borrow(&mut allocation, &graph, &limits);
    let before = capture(Box::from([owner.clone()]), &mut account).unwrap();
    before.require(&mut account).unwrap();
    fs::write(&owner, "[package]\nworkspace='../second'\n").unwrap();
    assert!(before.require(&mut account).is_err());
    fs::write(&owner, source).unwrap();
    before.require(&mut account).unwrap();
    fs::remove_file(&selected).unwrap();
    symlink(fixture.0.join("second"), &selected).unwrap();
    assert!(
        before.require(&mut account).is_err(),
        "identical target bytes do not hide retargeting"
    );
    fs::remove_file(&selected).unwrap();
    symlink(fixture.0.join("first"), &selected).unwrap();
    before.require(&mut account).unwrap();
    let ancestor = fixture.file("Cargo.toml", "[workspace]\n");
    assert!(before.require(&mut account).is_err());
    fs::remove_file(ancestor).unwrap();
    before.require(&mut account).unwrap();
    fs::write(first, "[workspace]\nresolver='3'\n").unwrap();
    assert!(before.require(&mut account).is_err());
}

#[test]
fn symlink_parent_redirect_observes_physical_and_lexical_targets() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let owner = fixture.file("member/Cargo.toml", "[package]\nworkspace='link/..'\n");
    let physical = fixture.file("external/Cargo.toml", "[workspace.package]\nversion='1'\n");
    fs::create_dir_all(fixture.0.join("external/child")).unwrap();
    symlink(
        fixture.0.join("external/child"),
        fixture.0.join("member/link"),
    )
    .unwrap();
    let before = image(std::slice::from_ref(&owner));
    fs::write(&physical, "[workspace.package]\nversion='2'\n").unwrap();
    assert!(before.entry != image(std::slice::from_ref(&owner)).entry);
    // The lexical alias points back to the owner. That recursive redirect must
    // terminate while retaining the raw spelling's external physical target.
    let Entry::Workspace { paths, .. } = &before.entry else {
        panic!("workspace entry")
    };
    assert!(paths
        .iter()
        .any(|path| Path::new(path.as_ref()) == physical));
    assert!(paths.iter().any(|path| Path::new(path.as_ref()) == owner));
}

#[test]
fn absent_component_before_parent_is_observed_beside_lexical_alias() {
    let fixture = Fixture::new();
    let owner = fixture.file(
        "member/Cargo.toml",
        "[package]\nworkspace='missing/../../workspace'\n",
    );
    let selected = fixture.file("workspace/Cargo.toml", "[workspace.package]\nversion='1'\n");
    let before = image(std::slice::from_ref(&owner));
    fs::write(selected, "[workspace.package]\nversion='2'\n").unwrap();
    assert!(before.entry != image(std::slice::from_ref(&owner)).entry);
    let before = image(std::slice::from_ref(&owner));
    fs::create_dir(fixture.0.join("member/missing")).unwrap();
    assert!(before.entry != image(std::slice::from_ref(&owner)).entry);
}

#[test]
fn workspace_parser_depth_refusal_keeps_original_reached_account() {
    let fixture = Fixture::new();
    let owner = fixture.file(
        "member/Cargo.toml",
        "ignored=[[[1]]]\n[package]\nworkspace='../selected'\n",
    );
    let mut limits = cargo::Limits::standard();
    limits.depth = 2;
    let graph = node::GraphBudget::new();
    let mut allocation = Allocation::for_replay(&limits);
    assert!(matches!(
        capture(
            Box::from([owner]),
            &mut Account::borrow(&mut allocation, &graph, &limits),
        ),
        Err(Error::Limit {
            resource: "workspace TOML depth",
            limit: 2,
            actual: 3
        })
    ));
    assert!(allocation.usage().items > 0);
    assert!(allocation.usage().bytes > 0);
}

#[test]
fn current_roots_include_transitive_path_and_git_owners() {
    use crate::cargo::unit::{Metadata, Package, PackageSource};
    let fixture = Fixture::new();
    let path_owner = fixture.file(
        "path/member/Cargo.toml",
        "[package]\nversion.workspace=true\n",
    );
    let git_owner = fixture.file(
        "git/member/Cargo.toml",
        "[package]\nversion.workspace=true\n",
    );
    let git_workspace = fixture.file("git/Cargo.toml", "[workspace.package]\nversion='1'\n");
    let metadata = Metadata::fixture_packages([
        Package::fixture(
            "a-harness",
            "harness",
            "0",
            fixture.0.join("harness"),
            PackageSource::Path {
                cargo_id: "path+harness".into(),
            },
        ),
        Package::fixture(
            "path-owner",
            "path",
            "1",
            path_owner.parent().unwrap().to_path_buf(),
            PackageSource::Path {
                cargo_id: "path+member".into(),
            },
        ),
        Package::fixture(
            "git-owner",
            "git",
            "1",
            git_owner.parent().unwrap().to_path_buf(),
            PackageSource::Git {
                cargo_id: "git+member".into(),
                oid: "0".repeat(40).into(),
            },
        ),
        Package::fixture(
            "registry-owner",
            "registry",
            "1",
            fixture.0.join("registry"),
            PackageSource::Registry {
                cargo_id: "registry+member".into(),
                checksum: Some([0; 32]),
            },
        ),
    ]);
    let limits = cargo::Limits::standard();
    let graph = node::GraphBudget::new();
    let mut allocation = Allocation::for_replay(&limits);
    let mut account = Account::borrow(&mut allocation, &graph, &limits);
    let roots = roots(&[], &metadata, &mut account).unwrap();
    let mut expected = [path_owner, git_owner];
    expected.sort();
    assert_eq!(roots.as_ref(), expected.as_slice());
    let before = capture(roots, &mut account).unwrap();
    fs::write(git_workspace, "[workspace.package]\nversion='2'\n").unwrap();
    assert!(before.require(&mut account).is_err());
}

#[test]
fn workspace_slots_admit_growth_even_with_allocator_surplus() {
    let mut limits = cargo::Limits::standard();
    limits.items = 1;
    let graph = node::GraphBudget::new();
    let mut allocation = Allocation::for_replay(&limits);
    let mut slots = Slots {
        values: Vec::<u64>::with_capacity(8),
        admitted: 0,
    };
    let mut account = Account::borrow(&mut allocation, &graph, &limits);
    account.push(&mut slots, 1).unwrap();
    // The doubled replacement layout is charged in full to the original
    // account regardless of the allocator's surplus capacity; crossing the item
    // target records an exceedance instead of refusing the growth.
    account
        .push(&mut slots, 2)
        .expect("accounting targets never refuse growth");
    assert_eq!(slots.values.as_slice(), &[1, 2]);
    assert_eq!(slots.admitted, 2);
    assert_eq!(allocation.usage().items, 3);
    assert_eq!(allocation.usage().bytes, 3 * std::mem::size_of::<u64>());
    let items = allocation
        .exceedances()
        .axis(crate::input::INPUT_ITEMS_RESOURCE)
        .copied()
        .expect("the item target was crossed by the replacement layout");
    assert_eq!(
        (items.target, items.first_requested, items.high_water),
        (1, 3, 3)
    );
    assert!(allocation
        .exceedances()
        .axis(crate::input::INPUT_BYTES_RESOURCE)
        .is_none());
    allocation
        .items(0)
        .expect("an exceedance never latches the account");
}

#[test]
fn watched_paths_are_in_the_order_the_stored_entry_reads() {
    // A path's own order puts `a/rawr/…` before `a/rawr-home/…`, and text
    // order puts it after, because `-` sorts before `/`.
    let fixture = Fixture::new();
    let near = fixture.file("a/rawr/member/Cargo.toml", "[package]\nname='near'\n");
    let far = fixture.file("a/rawr-home/Cargo.toml", "[package]\nname='far'\n");
    let image = image(&[near, far]);
    let Entry::Workspace { paths, .. } = &image.entry else {
        panic!("workspace entry")
    };
    assert!(paths.windows(2).all(|pair| pair[0] < pair[1]), "{paths:?}");
}

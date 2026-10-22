use super::*;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "rawr-workspace-cargo-{}-{}",
            std::process::id(),
            TEMPORARY_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&root).unwrap();
        Self(fs::canonicalize(root).unwrap())
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn authority_capture_retains_the_interpreted_manifest_and_absent_lock() {
    let fixture = Fixture::new();
    let manifest = fixture.0.join("Cargo.toml");
    let old = b"[workspace]\nresolver='2'\n";
    fs::write(&manifest, old).unwrap();
    let limits = Limits::standard();
    let mut allocation = RequestAllocation::new(&limits);
    let authority = Authority::load(Some(&manifest), &mut allocation).unwrap();
    fs::write(&manifest, b"[workspace]\nresolver='3'\n").unwrap();
    let lock = fixture.0.join("Cargo.lock");
    fs::write(&lock, b"version = 4\n").unwrap();
    let inputs = authority_inputs(&authority, &[], &mut allocation).unwrap();
    assert_eq!(
        inputs
            .iter()
            .find(|input| input.path == manifest)
            .unwrap()
            .digest,
        Some(digest(old))
    );
    assert_eq!(
        inputs
            .iter()
            .find(|input| input.path == lock)
            .unwrap()
            .digest,
        None
    );
    for input in inputs {
        assert!(
            authority_input_digest(&input.path, limits.manifest_bytes).unwrap() != input.digest
        );
    }
}

#[test]
fn cached_consumption_authority_is_bound_to_the_original_workspace_image() {
    let fixture = Fixture::new();
    let manifest = fixture.0.join("Cargo.toml");
    fs::write(&manifest, b"[workspace]\nresolver='2'\n").unwrap();
    let limits = Limits::standard();
    let graph = crate::node::GraphBudget::new();
    let mut allocation = RequestAllocation::new(&limits);
    let image = crate::input::workspace::capture(
        Box::from([manifest.clone()]),
        &mut crate::input::workspace::Account::borrow(&mut allocation, &graph, &limits),
    )
    .unwrap();
    let first =
        workspace_node_authority(b"same Build bytes", &image, &mut allocation, &graph).unwrap();
    assert_ne!(first.as_ref(), b"same Build bytes");
    fs::write(&manifest, b"[workspace]\nresolver='3'\n").unwrap();
    assert!(image
        .require(&mut crate::input::workspace::Account::borrow(
            &mut allocation,
            &graph,
            &limits
        ))
        .is_err());
    let changed = crate::input::workspace::capture(
        Box::from([manifest]),
        &mut crate::input::workspace::Account::borrow(&mut allocation, &graph, &limits),
    )
    .unwrap();
    let second =
        workspace_node_authority(b"same Build bytes", &changed, &mut allocation, &graph).unwrap();
    assert_ne!(first, second);
}

#[test]
fn workspace_roots_use_input_roles_and_lock_bytes_keep_direct_freshness() {
    let fixture = Fixture::new();
    let manifest = fixture.0.join("workspace-policy.toml");
    fs::write(&manifest, b"[workspace]\nresolver='2'\n").unwrap();
    let lock = fixture.0.join("Cargo.lock");
    // Valid TOML with a lock version and a manifest-shaped redirect must not
    // make a lock input a workspace-discovery root.
    fs::write(&lock, b"version=4\n[project]\nworkspace='bogus'\n").unwrap();
    fs::create_dir(fixture.0.join("bogus")).unwrap();
    fs::write(fixture.0.join("bogus/Cargo.toml"), "[workspace]\n").unwrap();
    let limits = Limits::standard();
    let graph = crate::node::GraphBudget::new();
    let mut allocation = RequestAllocation::new(&limits);
    let canonical = canonical_authority(&manifest, &mut allocation).unwrap();
    assert_eq!(canonical, manifest);
    let authority = Authority::load(Some(&canonical), &mut allocation).unwrap();
    let inputs = authority_inputs(&authority, &[], &mut allocation).unwrap();
    let metadata = unit::Metadata::fixture_packages([unit::Package::fixture(
        "harness",
        "harness",
        "0",
        fixture.0.join("harness"),
        unit::PackageSource::Path {
            cargo_id: "path+harness".into(),
        },
    )]);
    let mut account = crate::input::workspace::Account::borrow(&mut allocation, &graph, &limits);
    let roots = crate::input::workspace::roots(&inputs, &metadata, &mut account).unwrap();
    assert_eq!(roots.as_ref(), std::slice::from_ref(&manifest));
    let image = crate::input::workspace::capture(roots, &mut account).unwrap();
    let crate::input::proof::Entry::Workspace { paths, .. } = &image.entry else {
        panic!("workspace entry");
    };
    assert!(!paths.iter().any(|path| Path::new(path.as_ref()) == lock));
    assert!(!paths
        .iter()
        .any(|path| Path::new(path.as_ref()).starts_with(fixture.0.join("bogus"))));
    fs::write(&lock, b"version=4\n[project]\nworkspace='other'\n").unwrap();
    image.require(&mut account).unwrap();
    let original_lock = inputs
        .iter()
        .find(|input| input.role == Role::Lock)
        .unwrap();
    assert_ne!(
        authority_input_digest(&lock, limits.manifest_bytes).unwrap(),
        original_lock.digest
    );
    fs::write(&manifest, b"[workspace]\nresolver='3'\n").unwrap();
    assert!(image.require(&mut account).is_err());
}

#[test]
fn authority_inputs_preserve_manifest_and_lock_roles_at_the_same_path() {
    let fixture = Fixture::new();
    let manifest = fixture.0.join("Cargo.lock");
    fs::write(&manifest, b"[workspace]\nresolver='2'\n").unwrap();
    let limits = Limits::standard();
    let mut allocation = RequestAllocation::new(&limits);
    let authority = Authority::load(Some(&manifest), &mut allocation).unwrap();
    let inputs = authority_inputs(&authority, &[], &mut allocation).unwrap();
    assert_eq!(inputs.len(), 2);
    assert!(inputs.iter().all(|input| input.path() == manifest));
    assert_eq!(inputs.iter().filter_map(Input::manifest).count(), 1);
    assert!(inputs.iter().any(|input| input.role == Role::Lock));
}

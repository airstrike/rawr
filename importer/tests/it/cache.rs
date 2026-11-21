use crate::p3;

use std::fs;

#[test]
fn sealed_warm_entry_is_reused_pinned_and_explicitly_deletable() {
    let project = rawr_import::store::TestProject::new("rawr-p3-cache");
    let root = project.path();
    let first = p3::generated_at(root);
    let warm = p3::generated_at(root);
    let key = first.generated_manifest().build_request_key();
    let cache = rawr_import::cache::Writer::new(root).expect("the fixed cache writer binds");
    let mut lane = crate::shared::lane();
    let cold = first
        .build(&mut lane, &cache)
        .expect("the cold entry builds");
    let executable = fs::read_dir(cache.root())
        .expect("the cache root is readable")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.is_dir())
        .expect("one built entry exists")
        .join("target/debug/rawr-generated-runner");
    let executable_modified = fs::metadata(&executable)
        .expect("the executable is inspectable")
        .modified()
        .expect("the executable timestamp exists");

    let warm = warm
        .build(&mut lane, &cache)
        .expect("the complete warm evidence revalidates without rebuilding");
    assert_eq!(
        fs::metadata(&executable)
            .expect("the warm executable is inspectable")
            .modified()
            .expect("the warm timestamp exists"),
        executable_modified
    );
    let entries = cache.inspect().expect("the cache is inspectable");
    assert_eq!(entries.len(), 1);
    assert!(entries[0].sealed());
    assert!(entries[0].pinned());
    assert!(matches!(
        cache.delete(key),
        Err(rawr_import::cache::entry::Error::PinnedFull)
    ));

    drop(warm);
    drop(cold);
    assert!(cache.delete(key).expect("the unpinned entry is deletable"));
    assert!(cache
        .inspect()
        .expect("the empty cache is inspectable")
        .is_empty());
    drop(lane);
}

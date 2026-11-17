use std::fs;

fn bytes(seed: u64, length: usize) -> Vec<u8> {
    let mut state = seed;
    (0..length)
        .map(|_| {
            state ^= state << 13;
            state ^= state >> 7;
            state ^= state << 17;
            state as u8
        })
        .collect()
}

fn parts(directory: &std::path::Path) -> Vec<(std::path::PathBuf, std::time::SystemTime)> {
    let mut parts = fs::read_dir(directory)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            let modified = fs::metadata(&path).unwrap().modified().unwrap();
            (path, modified)
        })
        .collect::<Vec<_>>();
    parts.sort();
    parts
}

#[test]
fn two_generations_share_every_part_but_the_edited_ones_and_each_is_verified() {
    let directory = std::env::temp_dir().join(format!("rawr-parts-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    let original = bytes(7, 4 * 1024 * 1024);
    let manifest = super::write(&directory, &original).unwrap();
    assert_eq!(&*super::read(&directory, &manifest).unwrap(), &original[..]);
    let before = parts(&directory);
    assert!(before.len() > 8, "the bytes are cut into parts");

    // An insertion in the middle moves every later byte.
    let mut edited = original.clone();
    edited.splice(2_000_000..2_000_010, *b"an edit that grows the value");
    let edited_manifest = super::write(&directory, &edited).unwrap();
    assert_eq!(
        &*super::read(&directory, &edited_manifest).unwrap(),
        &edited[..]
    );
    let after = parts(&directory);
    let added = after.iter().filter(|part| !before.contains(part)).count();
    assert!(
        added <= 2,
        "only the edited region's parts are new: {added}"
    );
    assert!(
        before.iter().all(|part| after.contains(part)),
        "kept parts are untouched"
    );
    assert_eq!(
        super::write(&directory, &original).unwrap(),
        manifest,
        "the manifest is a function of the bytes"
    );
    super::present(&directory, &edited_manifest).expect("every part is kept");

    // A missing or altered part refuses the value rather than misreading it.
    let (victim, _) = &after[after.len() / 2];
    let mut permissions = fs::metadata(victim).unwrap().permissions();
    // Owner write is enough to alter it; nothing else needs access.
    let mode = std::os::unix::fs::PermissionsExt::mode(&permissions) | 0o200;
    std::os::unix::fs::PermissionsExt::set_mode(&mut permissions, mode);
    fs::set_permissions(victim, permissions).unwrap();
    fs::write(victim, b"altered").unwrap();
    assert!(
        super::read(&directory, &manifest).is_err()
            || super::read(&directory, &edited_manifest).is_err()
    );
    fs::remove_file(victim).unwrap();
    assert!(
        super::present(&directory, &manifest).is_err()
            || super::present(&directory, &edited_manifest).is_err()
    );
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_sources_index_names_each_file_and_its_parts() {
    let directory = std::env::temp_dir().join(format!("rawr-parts-sources-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    let library = bytes(3, 300 * 1024);
    let main = b"fn main() {}".to_vec();
    let digest =
        |bytes: &[u8]| -> [u8; 32] { <sha2::Sha256 as sha2::Digest>::digest(bytes).into() };
    let files = [
        ("src/lib.rs", &library[..], digest(&library)),
        ("src/main.rs", &main[..], digest(&main)),
    ];
    let index = super::sources::write(&directory, files.into_iter()).unwrap();
    let entries = super::sources::entries(&index).unwrap();
    assert_eq!(entries.len(), 2);
    for (entry, (path, bytes, digest)) in entries.iter().zip(files) {
        assert_eq!(entry.path, path);
        assert_eq!(entry.length, bytes.len() as u64);
        assert_eq!(entry.digest, digest);
        assert_eq!(&*super::read(&directory, entry.manifest).unwrap(), bytes);
    }
    assert_eq!(super::sources::manifests(&index).count(), 2);
    assert!(super::sources::entries(&index[..index.len() - 1]).is_err());
    assert_eq!(super::sources::manifests(&index[..9]).count(), 0);
    fs::remove_dir_all(directory).unwrap();
}

// A value whose entries each repeat one long field, as an API's definitions
// each repeat their compiler unit, still has boundaries throughout, so a new
// entry rewrites the parts around it and no others.
#[test]
fn an_entry_added_among_entries_that_repeat_a_long_field_rewrites_only_its_neighbours() {
    let directory = std::env::temp_dir().join(format!("rawr-parts-repeat-{}", std::process::id()));
    let _ = fs::remove_dir_all(&directory);
    let field = vec![b'u'; 500];
    let entry = |index: u64| [&field[..], &bytes(index, 40)].concat();
    let value = |entries: &[u64]| {
        entries
            .iter()
            .flat_map(|&index| entry(index))
            .collect::<Vec<u8>>()
    };
    let mut entries: Vec<u64> = (1..=8_000).collect();
    let manifest = super::write(&directory, &value(&entries)).unwrap();
    let before = parts(&directory);
    entries.insert(4_000, 1_000_000);
    let edited = super::write(&directory, &value(&entries)).unwrap();
    let added = parts(&directory)
        .iter()
        .filter(|part| !before.contains(part))
        .count();
    assert!(added <= 2, "an entry rewrote {added} parts");
    assert_ne!(manifest, edited);
    fs::remove_dir_all(directory).unwrap();
}

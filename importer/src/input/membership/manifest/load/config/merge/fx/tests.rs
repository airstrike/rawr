use super::Build;
use std::hash::BuildHasher;

#[cfg(all(
    target_pointer_width = "64",
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
#[test]
fn native_path_hashes_match_unmodified_pinned_dependency() {
    mod scenario {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/reference/manifest/config-fx/path_scenario.rs"
        ));
    }
    let mut expected = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/reference/manifest/config-fx/path-observations.txt"
    ))
    .lines();
    for path in scenario::paths() {
        let hash = u64::from_str_radix(expected.next().unwrap(), 16).unwrap();
        assert_eq!(Build.hash_one(path.as_path()), hash, "{path:?}");
        assert_eq!(Build.hash_one(path), hash);
    }
    assert!(expected.next().is_none());
}

// Observed from the pinned dependency on aarch64 with the pinned toolchain.
// The reference's widening-multiply and usize branches also govern x86_64.
#[cfg(all(
    target_pointer_width = "64",
    any(target_arch = "aarch64", target_arch = "x86_64")
))]
#[test]
fn string_hashes_match_pinned_dependency_at_every_short_length_and_unicode() {
    let mut expected = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/reference/manifest/config-fx/observations.txt"
    ))
    .lines();
    let mut check = |key: &str| {
        let hash = u64::from_str_radix(expected.next().unwrap(), 16).unwrap();
        assert_eq!(Build.hash_one(key), hash, "key {key:?}");
        // Cargo owns String keys; borrowed lookup must use the same encoding.
        assert_eq!(Build.hash_one(key.to_owned()), hash, "owned key {key:?}");
    };
    for length in 0..=1024 {
        let key: String = (0..length).map(|i| char::from((i % 127) as u8)).collect();
        check(&key);
    }
    for key in [
        "path-bases",
        "credential-provider",
        "é",
        "ß",
        "🦀",
        "a\0b",
        "日本語",
        "e\u{301}",
    ] {
        check(key);
    }
    assert!(expected.next().is_none());
}

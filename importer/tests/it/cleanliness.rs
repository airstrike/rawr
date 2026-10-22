use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use sha2::{Digest as _, Sha256};

#[test]
fn unchanged_freshness_fixtures_match_their_content_oracle() {
    let actual = fixture_oracle();
    assert_eq!(actual, expected());
    assert!(actual.keys().all(|path| {
        !path
            .components()
            .any(|component| matches!(component.as_os_str().to_str(), Some("target" | ".rawr")))
    }));
}

fn expected() -> BTreeMap<PathBuf, [u8; 32]> {
    [
        (
            "freshness-api/Cargo.toml",
            "95fc6c19a66896a037aafcd757b2f84af61dfdde364e7726419f5f0bcb036d72",
        ),
        (
            "freshness-api/build.rs",
            "671e1857a0448d12ae6f4128e1d905fa84dff482ce4375d7f0009e65bbabc7f2",
        ),
        (
            "freshness-api/helper.rs",
            "467cad125aac0b13bd0540b0170043b9431d6735496413c86165d524086ec947",
        ),
        (
            "freshness-api/src/lib.rs",
            "f975656efb65449c51234858d7b36fd31728a39e9c45a0f9aa90e003310ccb22",
        ),
        (
            "freshness-api/watched/value.txt",
            "0f446b887aa1078dc5d8654ef25a8c0bf449a96e89a00aa37a4ab3754f01bc11",
        ),
        (
            "freshness-api/absolute.txt",
            "db7525aebe28ce382736a6b5f6b056f6e2f6a7a6682a25277be794e91b409a40",
        ),
        (
            "freshness-api/relative.txt",
            "93de268984f7d73995e24479da33176b61c36749526436f8db6c05b92e10500b",
        ),
        (
            "fallback-api/Cargo.toml",
            "67b217b12a21bcfd5eb0437b54fa260fdb58be96402e1456fb449bda2a52c173",
        ),
        (
            "fallback-api/build.rs",
            "f7eba9e7288efa156784214e5f3314b08100e20314e811240f7613762a6bb9ff",
        ),
        (
            "fallback-api/src/lib.rs",
            "b7b379de1d3b19fec0acc365cd67ee5741b79f16dd8f456d3c01e758b5034223",
        ),
        (
            "fallback-api/fallback.txt",
            "3ea57cd33d5cad24655b624446ecb04faf7df6a06c8b41b05b94bd4eeceab6ca",
        ),
        (
            "fallback-api/included.txt",
            "1b9e1da1b16b62c306023b57cc882526925d2b205b66940b3772f842de2e0db2",
        ),
        (
            "fallback-api/excluded.txt",
            "dfa10daa42ca95559ec12bfc04dd0cc3001e4bb22c8bc02e6c07a26aceacdcbe",
        ),
        (
            "fallback-api/.cargo_vcs_info.json",
            "e1f72243f2a382410d7b7b70d23544271a69a761c89a491f65d13645311c1481",
        ),
        (
            "fallback-api/Cargo.toml.orig",
            "1972af5e2d904f612a1ec14a1990fd182be4e8a252a333e3ff71aa8a9a371ad1",
        ),
        (
            "fallback-api/Cargo.lock",
            "4a966075cef36ac80cc9cf4446fea62f1314552cb15dbaa9352917592459fc64",
        ),
    ]
    .into_iter()
    .map(|(path, digest)| (PathBuf::from(path), decode(digest)))
    .collect()
}

fn decode(value: &str) -> [u8; 32] {
    let mut bytes = [0_u8; 32];
    for (index, pair) in value.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let pair = std::str::from_utf8(pair).expect("the fixture digest oracle is ASCII");
        bytes[index] = u8::from_str_radix(pair, 16).expect("the fixture digest oracle is hex");
    }
    bytes
}

fn fixture_oracle() -> BTreeMap<PathBuf, [u8; 32]> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
    let mut oracle = BTreeMap::new();
    for fixture in ["freshness-api", "fallback-api"] {
        collect(&root.join(fixture), &root, &mut oracle);
    }
    oracle
}

fn collect(path: &Path, root: &Path, oracle: &mut BTreeMap<PathBuf, [u8; 32]>) {
    let mut entries = fs::read_dir(path)
        .expect("the unchanged fixture directory is readable")
        .collect::<std::result::Result<Vec<_>, _>>()
        .expect("the unchanged fixture entries are readable");
    entries.sort_unstable_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        if entry
            .file_type()
            .expect("the unchanged fixture entry has a type")
            .is_dir()
        {
            collect(&path, root, oracle);
        } else {
            let relative = path
                .strip_prefix(root)
                .expect("the unchanged fixture remains under its root")
                .to_path_buf();
            let digest = Sha256::digest(fs::read(&path).expect("the fixture file is readable"));
            assert!(oracle.insert(relative, digest.into()).is_none());
        }
    }
}

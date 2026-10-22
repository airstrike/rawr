use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt as _;

use super::{Package, Table};
use crate::input::stored::Error;

fn package(id: &str) -> Package {
    let mut root = b"/fixture/".to_vec();
    root.extend_from_slice(id.as_bytes());
    root.push(0xff);
    let root = std::path::PathBuf::from(OsString::from_vec(root));
    Package {
        manifest: root.join("Cargo.toml"),
        build_script: Some(root.join("build.rs")),
        root,
    }
}

#[test]
fn cold_and_decoded_preserve_order_lookup_and_native_payloads() {
    let ids = ["a", "a.b", "a/b", "é"];
    let cold = Table::from_cold(
        ids.iter()
            .rev()
            .map(|id| ((*id).into(), package(id)))
            .collect(),
    );
    let decoded = Table::from_decoded(ids.iter().map(|id| ((*id).into(), package(id))).collect())
        .expect("strictly sorted inventory");
    assert_eq!(cold.len(), ids.len());
    assert_eq!(decoded.len(), ids.len());
    assert!(!decoded.is_empty());
    let mut iter = decoded.iter();
    for ((expected, left), id) in cold.iter().zip(ids) {
        assert_eq!(iter.size_hint(), (iter.len(), Some(iter.len())));
        let (actual, right) = iter.next().expect("matching decoded entry");
        assert_eq!(actual, expected);
        assert_eq!(actual.as_ref(), id);
        assert_eq!(right.manifest, left.manifest);
        assert_eq!(right.root, left.root);
        assert_eq!(right.build_script, left.build_script);
        assert!(std::ptr::eq(
            decoded.get(id).expect("decoded lookup"),
            right
        ));
        assert!(std::ptr::eq(cold.get(id).expect("cold lookup"), left));
        assert!(decoded.contains_key(id));
    }
    assert_eq!(iter.len(), 0);
    assert!(iter.next().is_none());
    for absent in ["", "a-", "a.c", "éé"] {
        assert!(cold.get(absent).is_none());
        assert!(!decoded.contains_key(absent));
    }
}

#[test]
fn both_transfers_preserve_existing_storage() {
    let cold: BTreeMap<Box<str>, Package> = BTreeMap::from([("package".into(), package("cold"))]);
    let (key, value) = cold.first_key_value().expect("cold entry");
    let pointers = (key.as_ptr(), value as *const Package);
    let cold = Table::from_cold(cold);
    let (key, value) = cold.first_key_value().expect("moved entry");
    assert_eq!((key.as_ptr(), value as *const Package), pointers);

    let decoded: Box<[(Box<str>, Package)]> = Box::from([("package".into(), package("decoded"))]);
    let pointers = (decoded[0].0.as_ptr(), &decoded[0].1 as *const Package);
    let decoded = Table::from_decoded(decoded).expect("admitted storage transfers");
    let (key, value) = decoded.first_key_value().expect("moved entry");
    assert_eq!((key.as_ptr(), value as *const Package), pointers);
}

#[test]
fn decoded_rejects_duplicate_or_reversed_keys_without_normalizing() {
    for ids in [["same", "same"], ["z", "a"]] {
        let values = ids.into_iter().map(|id| (id.into(), package(id))).collect();
        assert!(matches!(
            Table::from_decoded(values),
            Err(Error::Encoding(_))
        ));
    }
    let empty = Table::from_decoded(Box::default()).expect("empty inventory");
    assert!(empty.is_empty());
    assert_eq!(empty.iter().len(), 0);
    assert!(empty.get("package").is_none());
}

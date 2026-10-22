use super::{decode, Error, Inputs, HEADER, MAX_BYTES, VERSION};
use std::ffi::OsStr;
use std::fs;
use std::mem::size_of;
use std::os::unix::ffi::OsStrExt as _;
use std::path::{Path, PathBuf};

fn payload(paths: &[&[u8]], trees: &[&[u8]]) -> Vec<u8> {
    roles(paths, trees, &[])
}

fn roles(paths: &[&[u8]], trees: &[&[u8]], sources: &[&[u8]]) -> Vec<u8> {
    let mut bytes = vec![VERSION];
    bytes.extend_from_slice(&(paths.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(trees.len() as u32).to_le_bytes());
    bytes.extend_from_slice(&(sources.len() as u32).to_le_bytes());
    for path in paths.iter().chain(trees.iter()).chain(sources.iter()) {
        bytes.extend_from_slice(&(path.len() as u32).to_le_bytes());
        bytes.extend_from_slice(path);
    }
    bytes
}

#[test]
fn observation_roundtrip_preserves_non_utf8_absent_paths_and_trees() {
    let directory = std::env::temp_dir().join(format!(
        "rawr-observation-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    fs::create_dir(&directory).expect("create fixture");
    let directory = fs::canonicalize(directory).expect("canonical fixture");
    let missing = directory.join("missing/entry.rawr");
    let non_utf8 = directory.join(OsStr::from_bytes(b"missing/\xff"));
    let tree = directory.join("absent-tree");
    let inputs = Inputs::from_paths(
        [missing.clone(), non_utf8.clone()].into_iter().collect(),
        [tree.clone()].into_iter().collect(),
    );
    let bytes = inputs.encode_observation().expect("encode");
    let decoded = decode(&bytes).expect("decode").into_inputs();
    assert_eq!(decoded, inputs);
    assert!(decoded.contains(&missing));
    assert!(decoded.contains(&non_utf8));
    assert!(decoded.contains(&tree.join("new-file")));
    assert_eq!(decoded.directories(), std::slice::from_ref(&directory));
    assert!(!missing.exists());
    fs::remove_dir(directory).expect("remove fixture");
}

#[test]
fn observation_decoder_rejects_bad_schema_counts_lengths_and_trailing_bytes() {
    let valid = payload(&[b"/a"], &[b"/t"]);
    for cut in 0..valid.len() {
        assert!(decode(&valid[..cut]).is_err(), "prefix {cut}");
    }
    let mut wrong_version = valid.clone();
    wrong_version[0] = VERSION + 1;
    assert_eq!(decode(&wrong_version).unwrap_err(), Error::Malformed);
    let mut count = payload(&[], &[]);
    count[1..5].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(decode(&count).unwrap_err(), Error::Malformed);
    let mut length = valid.clone();
    length[HEADER..HEADER + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(decode(&length).unwrap_err(), Error::Malformed);
    let mut trailing = valid;
    trailing.push(0);
    assert_eq!(decode(&trailing).unwrap_err(), Error::Malformed);
}

#[test]
fn observation_rejects_non_normalized_or_unordered_paths_in_both_sets() {
    for path in [
        b"".as_slice(),
        b"relative",
        b"//a",
        b"/a/",
        b"/./a",
        b"/a/../b",
        b"/a//b",
        b"/a\0b",
    ] {
        assert_eq!(
            decode(&payload(&[path], &[])).unwrap_err(),
            Error::Malformed
        );
        assert_eq!(
            decode(&payload(&[], &[path])).unwrap_err(),
            Error::Malformed
        );
    }
    for paths in [[b"/b".as_slice(), b"/a"], [b"/a".as_slice(), b"/a"]] {
        assert_eq!(decode(&payload(&paths, &[])).unwrap_err(), Error::Malformed);
        assert_eq!(decode(&payload(&[], &paths)).unwrap_err(), Error::Malformed);
    }
    // One identity may intentionally be both exact and recursive.
    assert!(decode(&payload(&[b"/"], &[b"/"])).is_ok());
}

#[test]
fn observation_wire_and_retained_layout_caps_are_distinct() {
    assert_eq!(decode(&vec![0; MAX_BYTES + 1]).unwrap_err(), Error::Limit);
    // Valid normalized paths still cannot allocate a directory/path pair over
    // the retained budget, even when their wire representation is smaller.
    let mut path = vec![b'a'; MAX_BYTES / 2 + 1];
    path[0] = b'/';
    assert_eq!(decode(&payload(&[&path], &[])).unwrap_err(), Error::Limit);
    let length = MAX_BYTES / 2 - size_of::<PathBuf>();
    path.truncate(length);
    assert!(decode(&payload(&[&path], &[])).is_ok());
    path.push(b'a');
    assert_eq!(decode(&payload(&[&path], &[])).unwrap_err(), Error::Limit);
}

#[test]
fn observation_encoder_checks_bounds_order_and_omits_derived_directories() {
    let input = |paths: Box<[PathBuf]>| Inputs {
        paths,
        directories: Box::from([PathBuf::from("not-an-authoritative-identity")]),
        trees: Box::default(),
        sources: Box::default(),
    };
    assert_eq!(
        input(Box::from([PathBuf::from("/b"), PathBuf::from("/a")]))
            .encode_observation()
            .unwrap_err(),
        Error::Malformed
    );
    assert_eq!(
        input(Box::from([PathBuf::from("/a/../b")]))
            .encode_observation()
            .unwrap_err(),
        Error::Malformed
    );
    let mut oversized = vec![b'a'; MAX_BYTES];
    oversized[0] = b'/';
    assert_eq!(
        input(Box::from([
            Path::new(OsStr::from_bytes(&oversized)).to_path_buf()
        ]))
        .encode_observation()
        .unwrap_err(),
        Error::Limit
    );
    assert_eq!(
        input(Box::default())
            .encode_observation()
            .expect("empty")
            .as_ref(),
        payload(&[], &[])
    );
}

#[test]
fn observation_directory_reconstruction_uses_current_transport_state() {
    let directory = std::env::temp_dir().join(format!(
        "rawr-observation-rederive-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    fs::create_dir(&directory).expect("create fixture");
    let directory = fs::canonicalize(directory).expect("canonical fixture");
    let path = directory.join("later/file.rawr");
    let encoded = payload(&[path.as_os_str().as_bytes()], &[]);
    let decoded = decode(&encoded).expect("pure decode while parent absent");
    fs::create_dir(directory.join("later")).expect("create new parent");
    let inputs = decoded.into_inputs();
    assert_eq!(inputs.paths(), &[path]);
    assert_eq!(inputs.directories(), &[directory.join("later")]);
    fs::remove_dir(directory.join("later")).expect("remove parent");
    fs::remove_dir(directory).expect("remove fixture");
}

#[test]
fn observation_source_roles_roundtrip_and_reject_invalid_subsets() {
    let encoded = roles(&[b"/a.rawr", b"/rawr.toml"], &[b"/native"], &[b"/a.rawr"]);
    let inputs = decode(&encoded).unwrap().into_inputs();
    assert_eq!(inputs.source_paths(), &[PathBuf::from("/a.rawr")]);
    assert!(inputs.is_source_only(Path::new("/a.rawr")));
    assert!(!inputs.is_source_only(Path::new("/rawr.toml")));
    assert_eq!(inputs.encode_observation().unwrap().as_ref(), encoded);
    for encoded in [
        roles(&[b"/a"], &[], &[b"/b"]),
        roles(&[b"/a", b"/b"], &[], &[b"/b", b"/a"]),
        roles(&[b"/a"], &[], &[b"/a", b"/a"]),
        roles(&[b"/native/a"], &[b"/native"], &[b"/native/a"]),
        roles(&[b"/native"], &[b"/native"], &[b"/native"]),
    ] {
        assert_eq!(decode(&encoded).unwrap_err(), Error::Malformed);
    }
    // Native roots can be nested, or lexically adjacent without overlapping.
    assert!(decode(&roles(&[b"/z"], &[b"/native", b"/native/a"], &[b"/z"])).is_ok());
    assert!(decode(&roles(&[b"/native-b/a"], &[b"/native"], &[b"/native-b/a"])).is_ok());
    let mut legacy = payload(&[], &[]);
    legacy[0] = 1;
    assert_eq!(decode(&legacy).unwrap_err(), Error::Malformed);
}

#[test]
fn observation_source_role_retention_is_charged_before_allocation() {
    let mut path = vec![b'a'; MAX_BYTES / 3];
    path[0] = b'/';
    assert!(decode(&payload(&[&path], &[])).is_ok());
    assert_eq!(
        decode(&roles(&[&path], &[], &[&path])).unwrap_err(),
        Error::Limit
    );
    let mut count = payload(&[], &[]);
    count[9..13].copy_from_slice(&u32::MAX.to_le_bytes());
    assert_eq!(decode(&count).unwrap_err(), Error::Malformed);
}

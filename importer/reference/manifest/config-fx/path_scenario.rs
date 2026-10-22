pub fn paths() -> Vec<std::path::PathBuf> {
    use std::os::unix::ffi::OsStrExt;
    let mut paths = Vec::new();
    for bytes in [
        b"".as_slice(),
        b"/",
        b".",
        b"..",
        b"a/b",
        b"a//b",
        b"a/./b",
        b"ab",
        b"/a/b",
        b"/a/../b",
        b"/a/\xff.rs",
        b"/a/\0b",
    ] {
        paths.push(std::path::PathBuf::from(std::ffi::OsStr::from_bytes(bytes)));
    }
    for length in 0..=128 {
        paths.push(std::path::PathBuf::from(format!(
            "/a/{}/z.rs",
            "x".repeat(length)
        )));
    }
    paths
}

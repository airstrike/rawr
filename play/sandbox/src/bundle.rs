//! The files the playground's examples read with `include_bytes!`, shipped
//! inside this crate, so inside the `?play` server and rawr-web.
//!
//! A snippet reads no file: the server's files are not its own, and a
//! browser has none (`rawr::resolve::deny_files`). These are the one
//! exception, a fixed set pinned by SHA-256. `include_bytes!` and
//! `include_str!` resolve a path only when it is exactly one of [`FILES`],
//! as the example writes it, and give that file's shipped bytes. No path
//! reaches a disk, so a snippet that names any other path still gets the
//! denial.

/// One shipped file: the path an example's `include_bytes!` names, its
/// bytes, and their SHA-256.
pub struct File {
    /// The path as the example writes it.
    pub path: &'static str,
    /// The example the file belongs to.
    pub example: &'static str,
    /// The file's bytes.
    pub bytes: &'static [u8],
    /// The SHA-256 of `bytes`, in hex.
    pub sha256: &'static str,
}

/// iced's `solar_system` draws its sun, earth and moon from these
/// (`examples/solar_system/assets` at iced `be88873`).
pub const FILES: [File; 3] = [
    File {
        path: "../assets/sun.png",
        example: "solar_system",
        bytes: include_bytes!("../assets/solar_system/sun.png"),
        sha256: "5828e2476062e5d78dd138e62dde87eaa704fe4549738f55ecfe13ddd52ea0d5",
    },
    File {
        path: "../assets/earth.png",
        example: "solar_system",
        bytes: include_bytes!("../assets/solar_system/earth.png"),
        sha256: "9ca72217990204ecfbebda36c8814a5d006076c112c4637b7d7576b4dae5ac87",
    },
    File {
        path: "../assets/moon.png",
        example: "solar_system",
        bytes: include_bytes!("../assets/solar_system/moon.png"),
        sha256: "c84524180696762dfdc7efe4f4f025516fe2b0edee8273aace4202c11aee70a2",
    },
];

/// [`FILES`] as the table `rawr::resolve::bundle_files` reads.
static TABLE: [(&str, &[u8]); 3] = [
    (FILES[0].path, FILES[0].bytes),
    (FILES[1].path, FILES[1].bytes),
    (FILES[2].path, FILES[2].bytes),
];

/// Let programs in this process read [`FILES`] by their paths.
pub fn install() {
    rawr::resolve::bundle_files(&TABLE);
}

#[cfg(test)]
mod tests {
    use sha2::{Digest, Sha256};

    use super::FILES;

    #[test]
    fn each_file_is_the_one_its_hash_pins_and_each_path_is_one_file() {
        for file in &FILES {
            let digest = Sha256::digest(file.bytes);
            let hex = digest
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            assert_eq!(hex, file.sha256, "{}", file.path);
        }
        for (index, file) in FILES.iter().enumerate() {
            assert!(
                FILES[index + 1..]
                    .iter()
                    .all(|other| other.path != file.path),
                "{} is shipped twice",
                file.path
            );
        }
    }

    #[test]
    fn only_a_shipped_path_resolves_where_files_are_denied() {
        rawr::resolve::deny_files("?play has no files");
        let mut engine = rawr::Engine::new();
        // Each file is larger than a `?play` array literal may be.
        engine.set_max_array_size(64 * 1024);
        crate::install(&mut engine).unwrap();
        assert!(FILES.iter().all(|file| file.bytes.len() > 64 * 1024));
        let len = engine
            .eval::<rawr::INT>(r#"include_bytes!("../assets/moon.png").len() as i64"#)
            .unwrap();
        assert_eq!(len as usize, FILES[2].bytes.len());
        // As iced's solar_system reads it: through a function, as a slice.
        let len = engine
            .eval::<rawr::INT>(
                r#"
                struct Moon { size: i64 }
                impl Moon {
                    fn new() -> Moon {
                        Moon { size: measure(include_bytes!("../assets/moon.png").as_slice()) }
                    }
                }
                fn measure(bytes: &[u8]) -> i64 { bytes.len() as i64 }
                Moon::new().size
                "#,
            )
            .unwrap();
        assert_eq!(len as usize, FILES[2].bytes.len());
        for denied in [
            r#"include_bytes!("../assets/moon.png ").len() as i64"#,
            r#"include_bytes!("/etc/passwd").len() as i64"#,
            r#"include_bytes!("../../assets/moon.png").len() as i64"#,
            r#"include_str!("Cargo.toml").len() as i64"#,
        ] {
            let error = engine.eval::<rawr::INT>(denied).unwrap_err().to_string();
            assert!(error.contains("?play has no files"), "{denied}: {error}");
        }
    }
}

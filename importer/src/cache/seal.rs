//! Atomic authority marker for one fixed cache entry.

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::Path;

pub(crate) const MARKER: &str = ".rawr-sealed";

pub(crate) fn publish(
    entry: &Path,
    manifest: &[u8; 32],
    artifact: &[u8; 32],
) -> Result<(), super::entry::Error> {
    let temporary = entry.join(format!(".seal-{}.tmp", std::process::id()));
    let marker = entry.join(MARKER);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|source| super::entry::Error::Io {
            operation: "create temporary runner seal",
            path: temporary.clone(),
            source,
        })?;
    writeln!(file, "manifest={}", hex(manifest)).map_err(|source| super::entry::Error::Io {
        operation: "write runner manifest seal",
        path: temporary.clone(),
        source,
    })?;
    writeln!(file, "artifact={}", hex(artifact)).map_err(|source| super::entry::Error::Io {
        operation: "write runner artifact seal",
        path: temporary.clone(),
        source,
    })?;
    file.sync_all().map_err(|source| super::entry::Error::Io {
        operation: "sync runner seal",
        path: temporary.clone(),
        source,
    })?;
    fs::rename(&temporary, &marker).map_err(|source| super::entry::Error::Io {
        operation: "publish runner seal",
        path: marker,
        source,
    })
}

pub(crate) fn exists(entry: &Path) -> bool {
    entry.join(MARKER).is_file()
}

pub(crate) fn validate(
    entry: &Path,
    manifest: &[u8; 32],
    artifact: &[u8; 32],
) -> Result<(), super::entry::Error> {
    let path = entry.join(MARKER);
    let actual = fs::read_to_string(&path).map_err(|source| super::entry::Error::Io {
        operation: "read runner seal",
        path,
        source,
    })?;
    let expected = format!("manifest={}\nartifact={}\n", hex(manifest), hex(artifact));
    if actual != expected {
        return Err(super::entry::Error::SealDrift);
    }
    Ok(())
}

fn hex(bytes: &[u8; 32]) -> String {
    use std::fmt::Write as _;
    let mut output = String::with_capacity(64);
    for byte in bytes {
        let _ = write!(output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static TEST_ID: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn seal_collisions_never_validate_as_warm_authority() {
        let root = std::env::temp_dir().join(format!(
            "rawr-cache-seal-{}-{}",
            std::process::id(),
            TEST_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).expect("the seal test entry exists");
        let manifest = [3_u8; 32];
        let artifact = [4_u8; 32];
        publish(&root, &manifest, &artifact).expect("the exact seal publishes");
        validate(&root, &manifest, &artifact).expect("the exact seal validates");
        assert!(matches!(
            validate(&root, &[5; 32], &artifact),
            Err(super::super::entry::Error::SealDrift)
        ));
        assert!(matches!(
            validate(&root, &manifest, &[6; 32]),
            Err(super::super::entry::Error::SealDrift)
        ));
        let _ = fs::remove_dir_all(root);
    }
}

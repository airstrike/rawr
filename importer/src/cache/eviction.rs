//! Deterministic unpinned cache eviction toward fixed quotas; pinned entries
//! are never evicted and never block a write.

use std::fs;
use std::path::Path;

const ENTRIES: usize = 8;
const BYTES: u64 = 8 * 1024 * 1024 * 1024;

pub(crate) fn fit(root: &Path, incoming: u64) -> Result<(), super::entry::Error> {
    let mut entries = super::entry::directories(root)?;
    entries.sort_unstable();
    let mut bytes = entries.iter().try_fold(0_u64, |total, entry| {
        total
            .checked_add(super::entry::bytes(entry)?)
            .ok_or(super::entry::Error::Quota)
    })?;
    while entries.len() >= ENTRIES || bytes.checked_add(incoming).is_none_or(|sum| sum > BYTES) {
        let Some(index) = entries
            .iter()
            .position(|entry| !super::pins::present(entry).unwrap_or(true))
        else {
            break;
        };
        let entry = entries.remove(index);
        let removed = super::entry::bytes(&entry)?;
        fs::remove_dir_all(&entry).map_err(|source| super::entry::Error::Io {
            operation: "evict runner cache entry",
            path: entry,
            source,
        })?;
        bytes = bytes.saturating_sub(removed);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs::{self, File};
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::*;

    static TEST_ID: AtomicU64 = AtomicU64::new(1);

    #[test]
    fn quotas_keep_pins_then_evict_lexically_and_by_measured_bytes() {
        let root = temporary("entries");
        fs::create_dir(&root).expect("the eviction root exists");
        for index in 0..ENTRIES {
            let entry = root.join(format!("entry-{index:02}"));
            fs::create_dir(&entry).expect("the synthetic entry exists");
            fs::write(entry.join(format!(".pin-{index}")), []).expect("the pin exists");
        }
        fit(&root, 1).expect("a fully pinned cache still admits the write");
        assert_eq!(
            super::super::entry::directories(&root)
                .expect("the pinned cache remains inspectable")
                .len(),
            ENTRIES
        );

        fs::remove_file(root.join("entry-00/.pin-0")).expect("one entry becomes evictable");
        fit(&root, 1).expect("the lexical first evictable entry is removed");
        assert!(!root.join("entry-00").exists());
        assert!(root.join("entry-01").is_dir());
        let _ = fs::remove_dir_all(&root);

        let root = temporary("bytes");
        let entry = root.join("oversized");
        fs::create_dir_all(&entry).expect("the byte-quota entry exists");
        File::create(entry.join("sparse"))
            .and_then(|file| file.set_len(BYTES))
            .expect("the sparse quota file is sized");
        fit(&root, 1).expect("the measured-byte entry is evicted");
        assert!(!entry.exists());
        let _ = fs::remove_dir_all(root);
    }

    fn temporary(label: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "rawr-cache-eviction-{label}-{}-{}",
            std::process::id(),
            TEST_ID.fetch_add(1, Ordering::Relaxed)
        ))
    }
}

//! The project's remembered crate demand, `<project>/.rawr/cache/demand`.
//!
//! Each line is one Rust path Rawr names through a wholesale crate re-export
//! (`iced::futures::stream`). Every preparation starts from this set, so a
//! demanded path is discovered once, not on every launch. The file is sorted
//! and deduplicated; a missing or malformed file is an empty set.

use std::path::{Path, PathBuf};

fn file(project: &Path) -> PathBuf {
    project.join(".rawr/cache/demand")
}

/// The remembered demand, sorted; empty when the file is missing or malformed.
#[must_use]
pub fn load(project: &Path) -> Vec<Box<str>> {
    let Ok(text) = std::fs::read_to_string(file(project)) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    for line in text.lines() {
        let valid = line.split("::").count() >= 2
            && line.split("::").all(|segment| {
                !segment.is_empty()
                    && segment
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
            });
        if !valid {
            return Vec::new();
        }
        paths.push(Box::from(line));
    }
    paths.sort_unstable();
    paths.dedup();
    paths
}

/// Replace the remembered demand with `paths`, sorted, written atomically.
/// A write failure only costs the next preparation one rediscovery.
pub fn store(project: &Path, paths: &[Box<str>]) {
    let mut sorted = paths.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    let file = file(project);
    let temporary = file.with_extension("partial");
    let mut text = String::new();
    for path in &sorted {
        text.push_str(path);
        text.push('\n');
    }
    let written = file
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(&temporary, text))
        .and_then(|()| std::fs::rename(&temporary, &file));
    if written.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
}

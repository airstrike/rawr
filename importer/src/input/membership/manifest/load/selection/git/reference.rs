// Copyright the gitoxide contributors. MIT OR Apache-2.0.
// Unchanged pinned normalize function; keep this file out of rustfmt.
use std::{borrow::Cow, path::{Path, PathBuf}};

pub fn normalize<'a>(path: Cow<'a, Path>, current_dir: &Path) -> Option<Cow<'a, Path>> {
    use std::path::Component::ParentDir;

    if !path.components().any(|c| matches!(c, ParentDir)) {
        return Some(path);
    }
    let mut current_dir_opt = Some(current_dir);
    let was_relative = path.is_relative();
    let components = path.components();
    let mut path = PathBuf::new();
    for component in components {
        if let ParentDir = component {
            let path_was_dot = path == Path::new(".");
            if path.as_os_str().is_empty() || path_was_dot {
                path.push(current_dir_opt.take()?);
            }
            if !path.pop() {
                return None;
            }
        } else {
            path.push(component);
        }
    }

    if (path.as_os_str().is_empty() || path == current_dir) && was_relative {
        Cow::Borrowed(Path::new("."))
    } else {
        path.into()
    }
    .into()
}

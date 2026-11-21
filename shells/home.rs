//! A path as a person reads it: one under the home directory starts with
//! `~`, as shells and editors show it. Both debugger shells include this
//! file, and only what they show goes through it. The files, sockets and
//! session keep their absolute paths.

use std::path::Path;

/// `path` with the home directory (`$HOME`) written as `~`, or `path`
/// itself when it isn't under that directory.
#[must_use]
pub fn shorten(path: &str) -> String {
    match std::env::var_os("HOME") {
        Some(home) => shorten_from(path, Path::new(&home)),
        None => path.to_owned(),
    }
}

/// `path` with `home` written as `~`. Only whole components match, so
/// `/Users/ann` doesn't shorten `/Users/anna`.
fn shorten_from(path: &str, home: &Path) -> String {
    if home.as_os_str().is_empty() || home == Path::new("/") {
        return path.to_owned();
    }
    match Path::new(path).strip_prefix(home) {
        Ok(rest) if rest.as_os_str().is_empty() => "~".to_owned(),
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_path_under_home_starts_with_a_tilde() {
        let home = Path::new("/Users/ann");
        assert_eq!(
            shorten_from("/Users/ann/demo/scores", home),
            "~/demo/scores"
        );
        assert_eq!(shorten_from("/Users/ann", home), "~");
        assert_eq!(shorten_from("/Users/anna/demo", home), "/Users/anna/demo");
        assert_eq!(shorten_from("/tmp/demo", home), "/tmp/demo");
        assert_eq!(shorten_from("/tmp/demo", Path::new("/")), "/tmp/demo");
    }
}

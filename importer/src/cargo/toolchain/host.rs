//! The host triple of a Rust toolchain, read from `rustc -vV`.
//!
//! The build script includes this file too, so the importer and its build
//! read the triple the same way.

/// The triple on the `host:` line of `rustc -vV` output.
pub(crate) fn parse(version: &str) -> Option<&str> {
    version
        .lines()
        .find_map(|line| line.strip_prefix("host:"))
        .map(str::trim)
        .filter(|host| !host.is_empty() && !host.contains(char::is_whitespace))
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn reads_the_host_line_of_any_platform() {
        let linux = "rustc 1.99.0-nightly (375b1431b 2026-07-10)\n\
            binary: rustc\n\
            commit-hash: 375b1431b7d89d1c2e2bc168c011848ae12b7d14\n\
            commit-date: 2026-07-10\n\
            host: x86_64-unknown-linux-gnu\n\
            release: 1.99.0-nightly\n\
            LLVM version: 21.1.0\n";
        assert_eq!(parse(linux), Some("x86_64-unknown-linux-gnu"));
        let mac = "rustc 1.99.0-nightly\nhost: aarch64-apple-darwin\r\nrelease: 1.99.0-nightly\n";
        assert_eq!(parse(mac), Some("aarch64-apple-darwin"));
        assert_eq!(
            parse("rustc 1.99.0-nightly\nrelease: 1.99.0-nightly\n"),
            None
        );
        assert_eq!(parse("host: \n"), None);
    }

    #[test]
    fn the_importer_host_is_the_toolchain_host() {
        // The build script read the triple from the rustc that compiled
        // this test, which is the toolchain on PATH here.
        let rustc = std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into());
        let output = std::process::Command::new(rustc)
            .arg("-vV")
            .output()
            .expect("rustc -vV runs");
        let version = String::from_utf8(output.stdout).expect("rustc -vV is UTF-8");
        assert_eq!(Some(super::super::HOST), parse(&version));
    }
}

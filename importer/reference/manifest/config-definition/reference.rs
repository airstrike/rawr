include!("definition.rs");
fn main() {
    let cwd = Path::new("/project");
    for (label, definition) in [
        ("normal-file", Definition::Path("/project/.cargo/config.toml".into())),
        ("root-file", Definition::Path("/config.toml".into())),
        ("normal-cli-file", Definition::Cli(Some("/tmp/config.toml".into()))),
        ("root-cli-file", Definition::Cli(Some("/config.toml".into()))),
        ("inline-cli", Definition::Cli(None)),
        ("environment", Definition::Environment("CARGO_PATH_BASES_WORKSPACE".into())),
        ("builtin", Definition::BuiltIn),
    ] {
        let result = std::panic::catch_unwind(|| definition.root(cwd).join("child"));
        match result {
            Ok(path) => println!("{label}: root-path={}", path.display()),
            Err(_) => println!("{label}: panicked"),
        }
    }
}

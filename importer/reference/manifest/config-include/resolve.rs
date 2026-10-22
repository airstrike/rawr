// Copyright the Cargo contributors. MIT OR Apache-2.0.
// ConfigInclude::resolve_path lexical prefix, with cwd passed directly instead
// of the inert GlobalContext accessor. No existence predicate in this reference.
fn resolved(def: &Definition, path: &std::path::Path, cwd: &std::path::Path) -> std::path::PathBuf {
    let abs_path = match def {
        Definition::Path(p) | Definition::Cli(Some(p)) => p.parent().unwrap(),
        Definition::Environment(_) | Definition::Cli(None) | Definition::BuiltIn => cwd,
    }
    .join(path);
    paths::normalize_path(&abs_path)
}

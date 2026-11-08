use rawr::project::{self, Target};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct Temp {
    path: PathBuf,
}

impl Temp {
    fn new(label: &str) -> Self {
        let nonce = NEXT_TEMP.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("rawr-project-{label}-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&path).unwrap();
        Self { path }
    }

    fn path(&self) -> &Path {
        &self.path
    }

    fn directory(&self, relative: &str) -> PathBuf {
        let path = self.path.join(relative);
        fs::create_dir_all(&path).unwrap();
        path
    }

    fn file(&self, relative: &str, contents: impl AsRef<[u8]>) -> PathBuf {
        let path = self.path.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&path, contents).unwrap();
        path
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// A marked crate at `root` whose `src/main.rs` holds `// rawr` then `contents`.
fn source(temp: &Temp, root: &str, contents: &str) -> PathBuf {
    temp.file(&format!("{root}/Cargo.toml"), format!("[package]\nname = \"{}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n", root.rsplit('/').next().unwrap()));
    temp.file(&format!("{root}/src/main.rs"), format!("// rawr\n{contents}"))
}

fn project(target: Target) -> project::Project {
    match target {
        Target::Project(project) => project,
        Target::File(file) => panic!("expected project, got file `{}`", file.path().display()),
    }
}

#[test]
fn explicit_source_file_never_ascends() {
    let temp = Temp::new("explicit-file");
    source(&temp, "outer", "outer");
    let explicit = temp.file("outer/examples/counter.rs", "explicit");

    let target = Target::discover(&explicit).unwrap();
    let Target::File(file) = target else {
        panic!("explicit source was promoted to a project");
    };

    assert_eq!(file.path(), fs::canonicalize(explicit).unwrap());
    assert_eq!(file.text(), "explicit");
}

#[test]
fn exact_marked_crate_directory_is_a_project() {
    let temp = Temp::new("exact-crate");
    source(&temp, "counter", "42");
    let root = temp.path().join("counter");

    let discovered = project(Target::discover(&root).unwrap());

    assert_eq!(discovered.root().as_path(), fs::canonicalize(root).unwrap());
    assert_eq!(discovered.source().text(), "// rawr\n42");
}

#[test]
fn a_marked_lib_rs_is_the_root_without_main_rs() {
    let temp = Temp::new("lib-root");
    temp.file("kit/Cargo.toml", "[package]\nname = \"kit\"\n");
    let lib = temp.file("kit/src/lib.rs", "// rawr\npub fn f() {}\n");

    let discovered = project(Target::discover(temp.path().join("kit")).unwrap());

    assert_eq!(discovered.source().path(), fs::canonicalize(lib).unwrap());
}

#[test]
fn nested_directory_finds_one_marked_crate() {
    let temp = Temp::new("nested-crate");
    source(&temp, "counter", "nested");
    let nested = temp.directory("counter/src/components/deep");

    let discovered = project(Target::discover(nested).unwrap());

    assert_eq!(discovered.root().as_path(), fs::canonicalize(temp.path().join("counter")).unwrap());
    assert_eq!(discovered.source().text(), "// rawr\nnested");
}

#[test]
fn explicit_cargo_toml_selects_its_exact_crate() {
    let temp = Temp::new("explicit-manifest");
    source(&temp, "outer", "outer");
    source(&temp, "outer/inner", "inner");

    let discovered = project(Target::discover(temp.path().join("outer/Cargo.toml")).unwrap());

    assert_eq!(discovered.root().as_path(), fs::canonicalize(temp.path().join("outer")).unwrap());
    assert_eq!(discovered.source().text(), "// rawr\nouter");
}

#[test]
fn the_marker_follows_a_shebang_and_frontmatter() {
    let temp = Temp::new("lead");
    temp.file("tool/Cargo.toml", "[package]\nname = \"tool\"\n");
    temp.file("tool/src/main.rs", "#!/usr/bin/env rawr\n// rawr\nfn main() {}\n");

    let discovered = project(Target::discover(temp.path().join("tool")).unwrap());

    assert!(discovered.source().text().starts_with("#!"));
}

#[test]
fn a_frontmatter_crate_root_is_a_script_not_the_package() {
    let temp = Temp::new("script-root");
    temp.file("tool/Cargo.toml", "[package]\nname = \"tool\"\n\n[dependencies]\nrand = \"0.9\"\n");
    let main = temp.file("tool/src/main.rs", "---cargo\n---\n// rawr\nfn main() {}\n");
    let nested = temp.directory("tool/src/deep");

    for start in [temp.path().join("tool"), temp.path().join("tool/Cargo.toml"), nested] {
        let Target::File(file) = Target::discover(&start).unwrap() else {
            panic!("a script was folded into its package from `{}`", start.display());
        };
        assert_eq!(file.path(), fs::canonicalize(&main).unwrap());
    }
}

#[test]
fn an_unmarked_frontmatter_crate_root_is_unmarked() {
    let temp = Temp::new("unmarked-script");
    temp.file("tool/Cargo.toml", "[package]\nname = \"tool\"\n");
    // A blank line after the block: the marker is not the first line of Rust.
    temp.file("tool/src/main.rs", "---cargo\n---\n\n// rawr\nfn main() {}\n");

    let error = Target::discover(temp.path().join("tool")).unwrap_err();

    assert!(matches!(error, project::Error::Unmarked { .. }), "{error}");
}

#[test]
fn an_unmarked_crate_is_not_a_rawr_project() {
    let temp = Temp::new("unmarked");
    temp.file("plain/Cargo.toml", "[package]\nname = \"plain\"\n");
    let main = temp.file("plain/src/main.rs", "fn main() {}\n");

    let error = Target::discover(temp.path().join("plain")).unwrap_err();

    assert!(matches!(error, project::Error::Unmarked { ref entry, .. } if entry == &fs::canonicalize(&main).unwrap()));
    assert!(
        error
            .to_string()
            .ends_with("is not a Rawr file. Its first line is not `// rawr`, and no module it declares starts with it.")
    );
}

#[test]
fn an_unmarked_crate_that_declares_a_marked_module_is_a_rawr_project() {
    let temp = Temp::new("native-root");
    temp.file("app/Cargo.toml", "[package]\nname = \"app\"\n");
    let main = temp.file("app/src/main.rs", "mod sim;\nfn main() { sim::world::run(); }\n");
    temp.file("app/src/sim.rs", "pub mod world;\n");
    temp.file("app/src/sim/world.rs", "// rawr\npub fn run() {}\n");

    let discovered = project(Target::discover(temp.path().join("app")).unwrap());

    // Found through the native module `sim`; the entry is the native root.
    assert_eq!(discovered.source().path(), fs::canonicalize(&main).unwrap());
}

#[test]
fn a_package_without_a_target_is_cargos_error() {
    let temp = Temp::new("no-root");
    let manifest = temp.file("empty/Cargo.toml", "[package]\nname = \"empty\"\n");

    let error = Target::discover(manifest).unwrap_err();

    // Cargo describes the package, and says it has no target.
    assert!(matches!(error, project::Error::Cargo { .. }), "{error}");
    assert!(error.to_string().contains("no targets specified in the manifest"), "{error}");
}

#[test]
fn exact_candidate_directory_wins_over_an_outer_crate() {
    let temp = Temp::new("exact-wins");
    source(&temp, "outer", "outer");
    source(&temp, "outer/inner", "inner");
    let inner = temp.path().join("outer/inner");

    let discovered = project(Target::discover(&inner).unwrap());

    assert_eq!(discovered.root().as_path(), fs::canonicalize(inner).unwrap());
    assert_eq!(discovered.source().text(), "// rawr\ninner");
}

#[test]
fn nested_marked_crates_are_ambiguous() {
    let temp = Temp::new("ambiguous");
    source(&temp, "outer", "outer");
    source(&temp, "outer/inner", "inner");
    let start = temp.directory("outer/inner/components/deep");

    let error = Target::discover(&start).unwrap_err();
    let project::Error::Ambiguous { start: actual_start, candidates } = error else {
        panic!("expected ambiguity, got {error}");
    };

    assert_eq!(actual_start, fs::canonicalize(start).unwrap());
    assert_eq!(candidates.as_ref(), [fs::canonicalize(temp.path().join("outer/inner")).unwrap(), fs::canonicalize(temp.path().join("outer")).unwrap(),]);
}

#[test]
fn vcs_boundary_stops_ancestor_search_inclusively() {
    let temp = Temp::new("vcs-boundary");
    source(&temp, "outer", "must not be selected");
    temp.directory("outer/repository/.git");
    let start = temp.directory("outer/repository/components/deep");

    let error = Target::discover(&start).unwrap_err();
    let project::Error::NotFound { start: actual_start, boundary, legacy } = error else {
        panic!("expected not found, got {error}");
    };

    assert_eq!(actual_start, fs::canonicalize(start).unwrap());
    assert_eq!(boundary, fs::canonicalize(temp.path().join("outer/repository")).unwrap());
    assert_eq!(legacy, None);
}

#[test]
fn an_explicit_rawr_file_says_how_to_convert_it() {
    let temp = Temp::new("legacy-file");
    let path = temp.file("cart.rawr", "fn main() {}\n");

    let error = Target::discover(&path).unwrap_err();

    assert!(matches!(error, project::Error::Legacy { .. }));
    assert_eq!(
        error.to_string(),
        format!(
            "`{}` is a `.rawr` file. Rawr reads `.rs` files now. \
             Rename the file to `.rs` and put `// rawr` on its first line. \
             A project goes in a Cargo crate whose `src/main.rs` starts with `// rawr`.",
            fs::canonicalize(&path).unwrap().display()
        )
    );
}

#[test]
fn an_explicit_rawr_toml_says_how_to_convert_the_project() {
    let temp = Temp::new("legacy-manifest");
    temp.file("app/src/main.rawr", "fn main() {}\n");
    let path = temp.file("app/rawr.toml", "[package]\nname = \"app\"\n");

    let error = Target::discover(&path).unwrap_err();

    assert!(matches!(error, project::Error::Legacy { .. }));
    assert_eq!(
        error.to_string(),
        format!(
            "`{}` is no longer read. Rawr reads `.rs` files in a Cargo crate now. \
             Put the project in a Cargo crate whose `src/main.rs` starts with `// rawr`. \
             Rename each `.rawr` file to `.rs` and put `// rawr` on its first line. \
             Move the dependencies to the `[dependencies]` of `Cargo.toml`.",
            fs::canonicalize(&path).unwrap().display()
        )
    );
}

#[test]
fn a_search_that_passes_an_old_project_names_it() {
    let temp = Temp::new("legacy-search");
    temp.directory("app/.git");
    temp.file("app/rawr.toml", "[package]\nname = \"app\"\n");
    let main = temp.file("app/src/main.rawr", "fn main() {}\n");
    let start = temp.directory("app/src/view");

    let error = Target::discover(&start).unwrap_err();
    let project::Error::NotFound { ref legacy, .. } = error else {
        panic!("expected not found, got {error}");
    };

    let app = fs::canonicalize(temp.path().join("app")).unwrap();
    assert_eq!(legacy.as_deref(), Some(app.join("rawr.toml").as_path()));
    assert!(main.exists());
    assert_eq!(
        error.to_string(),
        format!(
            "no Rawr project found from `{}` through `{}`. \
             A Rawr project is a Cargo crate whose `src/main.rs` starts with `// rawr`. \
             `{}` is no longer read. \
             Rawr does not read rawr.toml or `.rawr` files anymore.",
            fs::canonicalize(&start).unwrap().display(),
            app.display(),
            app.join("rawr.toml").display()
        )
    );
}

#[test]
fn missing_input_has_a_positioned_error() {
    let temp = Temp::new("missing-input");
    let missing = temp.path().join("does-not-exist");

    let error = Target::discover(&missing).unwrap_err();

    assert!(matches!(
        error,
        project::Error::InputNotFound { ref path, .. } if path == &missing
    ));
    assert!(error.to_string().contains("the path does not exist"));
}

#[test]
fn invalid_utf8_source_is_unreadable() {
    let temp = Temp::new("invalid-utf8");
    let expected_path = temp.file("counter/single.rs", [0xff, 0xfe]);

    let error = Target::discover(&expected_path).unwrap_err();

    assert!(matches!(
        error,
        project::Error::Unreadable {
            resource: project::Resource::Source,
            ref path,
            ..
        } if path == &fs::canonicalize(expected_path).unwrap()
    ));
}

#[test]
fn unsupported_explicit_file_is_actionable() {
    let temp = Temp::new("unsupported");
    let path = temp.file("notes.txt", "not Rawr");

    let error = Target::discover(path).unwrap_err();

    assert!(matches!(error, project::Error::UnsupportedInput { .. }));
    assert!(error.to_string().ends_with("Pass a directory, a `.rs` file or a `Cargo.toml`."));
}

#[cfg(unix)]
#[test]
fn project_source_symlink_cannot_escape_root() {
    use std::os::unix::fs::symlink;

    let temp = Temp::new("source-escape");
    let outside = temp.file("outside.rs", "// rawr\noutside");
    temp.file("counter/Cargo.toml", "[package]\nname = \"counter\"\n");
    temp.directory("counter/src");
    symlink(outside, temp.path().join("counter/src/main.rs")).unwrap();

    let error = Target::discover(temp.path().join("counter")).unwrap_err();

    assert!(matches!(error, project::Error::EscapesRoot { resource: project::Resource::Source, .. }));
}

#[cfg(unix)]
#[test]
fn explicit_source_symlink_uses_its_canonical_target() {
    use std::os::unix::fs::symlink;

    let temp = Temp::new("explicit-symlink");
    let target = temp.file("sources/actual.rs", "actual");
    let link = temp.path().join("selected.rs");
    symlink(&target, &link).unwrap();

    let discovered = Target::discover(link).unwrap();
    let Target::File(file) = discovered else {
        panic!("explicit source symlink did not remain a file target");
    };

    assert_eq!(file.path(), fs::canonicalize(target).unwrap());
    assert_eq!(file.text(), "actual");
}

/// A package's manifest, its own workspace, with `rest` after `[package]`.
fn manifest(temp: &Temp, root: &str, rest: &str) -> PathBuf {
    temp.file(&format!("{root}/Cargo.toml"), format!("[package]\nname = \"{root}\"\nversion = \"0.1.0\"\nedition = \"2024\"\n{rest}\n[workspace]\n"))
}

/// A package whose only binary is `src/bin/tool.rs` runs from it, as
/// `cargo run` does (#1116).
#[test]
fn a_marked_binary_under_src_bin_is_the_packages_entry() {
    let temp = Temp::new("src-bin");
    manifest(&temp, "tools", "");
    let tool = temp.file("tools/src/bin/tool.rs", "// rawr\nfn main() {}\n");

    let discovered = project(Target::discover(temp.path().join("tools")).unwrap());

    assert_eq!(discovered.source().path(), fs::canonicalize(tool).unwrap());
    assert_eq!(discovered.package().unwrap().name(), "tools");
}

/// A file the package lists as a `[[bin]]` is that package's binary,
/// found by its path, and `--bin` finds it by its name (#1175).
#[test]
fn a_bin_target_file_is_its_packages_binary() {
    let temp = Temp::new("bin-target");
    manifest(&temp, "exercises", "\n[[bin]]\nname = \"async1\"\npath = \"lessons/async1.rs\"\n\n[[bin]]\nname = \"clippy3\"\npath = \"lessons/clippy3.rs\"\n");
    let async1 = temp.file("exercises/lessons/async1.rs", "// rawr\nfn main() {}\n");
    temp.file("exercises/lessons/clippy3.rs", "// rawr\nfn main() {}\n");
    let lone = temp.file("exercises/lessons/notes.rs", "// rawr\nfn main() {}\n");

    let discovered = project(Target::discover(&async1).unwrap());
    assert_eq!(discovered.root().as_path(), fs::canonicalize(temp.path().join("exercises")).unwrap());
    assert_eq!(discovered.source().path(), fs::canonicalize(&async1).unwrap());

    // A file no package lists still runs alone.
    assert!(matches!(Target::discover(&lone).unwrap(), Target::File(_)));

    let root = temp.path().join("exercises/lessons");
    assert_eq!(Target::binary(&root, "async1").unwrap(), fs::canonicalize(&async1).unwrap());
    let error = Target::binary(&root, "async2").unwrap_err();
    assert!(matches!(error, project::Error::NoBinary { .. }));
    assert_eq!(error.to_string(), "no bin target named `async2`\navailable bin targets:\n    async1\n    clippy3");
}

/// With several binaries and no `default-run`, which one runs is not
/// known, as `cargo run` says; `default-run` picks one.
#[test]
fn several_binaries_need_a_default_run() {
    let temp = Temp::new("several-bins");
    manifest(&temp, "pair", "");
    temp.file("pair/src/main.rs", "// rawr\nfn main() {}\n");
    temp.file("pair/src/bin/other.rs", "// rawr\nfn main() {}\n");

    let error = Target::discover(temp.path().join("pair")).unwrap_err();
    assert!(matches!(error, project::Error::AmbiguousBinary { .. }), "{error}");
    assert!(error.to_string().contains("available binaries: other, pair"), "{error}");

    manifest(&temp, "pair", "default-run = \"other\"\n");
    let discovered = project(Target::discover(temp.path().join("pair")).unwrap());
    assert_eq!(discovered.source().path(), fs::canonicalize(temp.path().join("pair/src/bin/other.rs")).unwrap());
}

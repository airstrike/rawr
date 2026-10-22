//! Static application checking without native window creation.

use std::path::PathBuf;

use crate::compile;

#[cfg(any(not(feature = "import-baseline"), test))]
pub(crate) fn execute(path: Option<PathBuf>) -> Result<PathBuf, compile::Error> {
    execute_transform(
        path,
        #[cfg(feature = "import-baseline")]
        None,
    )
}

#[cfg(feature = "import-baseline")]
pub(crate) fn execute_with_profile(
    path: Option<PathBuf>,
    profile: Option<rawr_iced::profile::Fixture>,
) -> Result<PathBuf, compile::Error> {
    execute_transform(path, profile)
}

fn execute_transform(
    path: Option<PathBuf>,
    #[cfg(feature = "import-baseline")] profile: Option<rawr_iced::profile::Fixture>,
) -> Result<PathBuf, compile::Error> {
    #[cfg(feature = "native-runner")]
    if let Some(target) =
        crate::import::declared(path.as_ref()).map_err(compile::Error::from_report)?
    {
        return crate::import::owner::check(
            &target,
            #[cfg(feature = "import-baseline")]
            profile,
        )
        .map_err(compile::Error::from_report);
    }
    #[cfg(feature = "import-baseline")]
    if profile.is_some() {
        return Err(compile::Error::from_report(
            rawr::diagnostic::Report::error(
                rawr::diagnostic::code::FRAMEWORK_NATIVE,
                "private profile selection requires a declared Rust import",
            )
            .finish(),
        ));
    }
    let validated = compile::validate(path)?;
    Ok(validated.source().to_path_buf())
}

/// `rawr check --as-rust`. A `.rawr` module is meant to be close enough
/// to Rust to rename later; this proves it for each one. The app's `src/` is
/// copied under `target/rawr-as-rust/` with every `.rawr` renamed to `.rs`,
/// beside its `Cargo.toml` (relative `path`s made absolute, its own
/// `[workspace]`), and `cargo check` runs there. The result names each
/// `.rawr` module with `ok` or its first error; it succeeds when all are ok.
pub(crate) fn as_rust(path: Option<PathBuf>) -> Result<(String, bool), compile::Error> {
    use rawr::diagnostic::{code, Report};
    use std::collections::BTreeMap;
    use std::path::Path;

    let fail = |message: String| {
        compile::Error::from_report(Report::error(code::ENVIRONMENT_DIAGNOSTIC, message).finish())
    };
    let target = match path {
        Some(path) => rawr::project::Target::discover(path),
        None => rawr::project::Target::current(),
    }
    .map_err(|error| compile::Error::from_report(Report::from_project(&error)))?;
    let graph = rawr::package::Graph::load(&rawr::Engine::new(), &target)
        .map_err(|error| fail(error.to_string()))?;
    let entry = graph.entry_package();
    // A Cargo package is the crate; a `rawr.toml` `[rust]` project (its
    // Cargo authority a workspace) gets one from `[rust.dependencies]`.
    let (root, manifest_text, lock) = match entry.cargo_manifest() {
        Some((manifest, text)) => {
            let root = manifest.parent().unwrap_or(entry.root()).to_path_buf();
            let lock = root.join("Cargo.lock");
            (root, text.to_owned(), lock)
        }
        None => {
            let generated = entry
                .manifest_text()
                .and_then(|text| rawr_toml_manifest(entry.name(), text, entry.root()));
            let Some((text, authority)) = generated else {
                return Err(fail(format!(
                    "`rawr check --as-rust` needs a Cargo.toml with a `[package]` or a rawr.toml `[rust.dependencies]` at {}",
                    entry.root().display()
                )));
            };
            let lock = authority.map_or_else(
                || entry.root().join("Cargo.lock"),
                |authority| authority.with_file_name("Cargo.lock"),
            );
            (entry.root().to_path_buf(), text, lock)
        }
    };
    let manifest_text = manifest_text.as_str();
    let module_root = graph
        .entry_source()
        .path()
        .parent()
        .unwrap_or(&root)
        .to_path_buf();
    let Ok(source_directory) = module_root.strip_prefix(&root).map(Path::to_path_buf) else {
        return Err(fail(format!(
            "{} is outside the Cargo package at {}",
            module_root.display(),
            root.display()
        )));
    };
    let modules = graph
        .sources()
        .iter()
        .filter(|source| source.path().starts_with(&module_root))
        .map(|source| source.path().to_path_buf())
        .collect::<Vec<_>>();

    let work = root.join("target/rawr-as-rust");
    let copy = work.join("crate");
    let io = |error: std::io::Error| fail(format!("rawr check --as-rust: {error}"));
    if copy.exists() {
        std::fs::remove_dir_all(&copy).map_err(io)?;
    }
    copy_renamed(&module_root, &copy.join(&source_directory)).map_err(io)?;
    let mut cargo_toml = as_rust_manifest(manifest_text, &root);
    // The iced Rawr supports needs Rawr's own `[patch]` entries; a
    // project with no `[patch]` of its own gets them, as the import does.
    // Rawr runs the app with its facade's iced features (an executor, a
    // renderer) unified into the app's iced; the copy gets them the same way.
    #[cfg(feature = "native-runner")]
    if let Ok(host) = rawr_import::host::root() {
        if !manifest_text
            .lines()
            .any(|line| line.trim_start().starts_with("[patch"))
            && let Ok(text) = std::fs::read_to_string(host.join("Cargo.toml"))
        {
            cargo_toml.push('\n');
            cargo_toml.push_str(rawr_patches(&text, &host).trim());
            cargo_toml.push('\n');
        }
        if let Ok(text) = std::fs::read_to_string(host.join("iced/Cargo.toml")) {
            cargo_toml = with_facade_iced(&cargo_toml, &text);
        }
    }
    std::fs::write(copy.join("Cargo.toml"), cargo_toml).map_err(io)?;
    if let Ok(lock) = std::fs::read(&lock) {
        std::fs::write(copy.join("Cargo.lock"), lock).map_err(io)?;
    }

    // The pinned `rustc` is `RUSTC`: through the rustup proxy, Cargo's rustc
    // for a git dependency (run in its checkout) would be the default
    // toolchain, and its artifacts would not load into the app's crate.
    // Artifacts are per toolchain, so is the target directory.
    #[cfg(feature = "native-runner")]
    let (cargo, rustc) = match rawr_import::process::Native::pinned() {
        Ok(native) => (
            native.cargo_executable().to_path_buf(),
            Some(native.rustc_executable().to_path_buf()),
        ),
        Err(_) => (PathBuf::from("cargo"), None),
    };
    #[cfg(not(feature = "native-runner"))]
    let (cargo, rustc) = (PathBuf::from("cargo"), None::<PathBuf>);
    let toolchain = rustc
        .as_deref()
        .and_then(|rustc| rustc.parent()?.parent()?.file_name())
        .map_or_else(|| "target".into(), |name| name.to_string_lossy());
    let mut command = std::process::Command::new(&cargo);
    if let Some(rustc) = &rustc {
        command.env("RUSTC", rustc);
    }
    let output = command
        .args([
            "check",
            "--quiet",
            "--message-format",
            "short",
            "--manifest-path",
        ])
        .arg(copy.join("Cargo.toml"))
        .env("CARGO_TARGET_DIR", work.join(&*toolchain))
        .current_dir(&copy)
        .output()
        .map_err(io)?;
    let stderr = String::from_utf8_lossy(&output.stderr);

    // `src/view.rs:3:5: error[E0425]: cannot find value `x` in this scope`
    let mut first = BTreeMap::<PathBuf, String>::new();
    for line in stderr.lines() {
        let Some((file, rest)) = line.split_once(':') else {
            continue;
        };
        if !rest.contains(": error") {
            continue;
        }
        let file = copy.join(file);
        let Ok(relative) = file.strip_prefix(&copy.join(&source_directory)) else {
            continue;
        };
        first
            .entry(module_root.join(relative))
            .or_insert_with(|| rest.to_owned());
    }
    let mut report = String::new();
    let mut clean = output.status.success();
    let reached = clean || !first.is_empty();
    if !reached {
        // The crate fails outside its modules (a dependency, the manifest).
        report.push_str("cargo check failed before reaching the modules:\n");
        for line in stderr.lines().filter(|line| line.contains("error")).take(5) {
            report.push_str(&format!("  {line}\n"));
        }
    }
    for module in &modules {
        let relative = module.strip_prefix(&root).unwrap_or(module);
        let renamed = module.with_extension("rs");
        match first.remove(&renamed) {
            Some(error) => {
                clean = false;
                report.push_str(&format!("{}:{error}\n", relative.display()));
            }
            None if reached => report.push_str(&format!("{}: ok\n", relative.display())),
            None => report.push_str(&format!("{}: not checked\n", relative.display())),
        }
    }
    for (file, error) in first {
        let relative = file.strip_prefix(&root).unwrap_or(&file);
        report.push_str(&format!("{}:{error}\n", relative.display()));
    }
    report.push_str(&format!(
        "{} as Rust: {}",
        root.display(),
        if clean {
            "every .rawr module compiles as .rs"
        } else {
            "not yet Rust"
        }
    ));
    Ok((report, clean))
}

/// Project shape: a `rawr.toml` whose Rust dependencies are
/// `[rust.dependencies]` and whose `cargo-authority` is a workspace manifest.
/// The crate's `Cargo.toml` is a `[package]` named after the Rawr package with
/// those dependencies (relative to `root`) and the authority's `[patch.*]`
/// tables (absolute against it). Also returns the authority's path.
fn rawr_toml_manifest(
    name: &str,
    text: &str,
    root: &std::path::Path,
) -> Option<(String, Option<PathBuf>)> {
    let mut dependencies = String::new();
    let mut authority = None;
    let mut section = "";
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            section = trimmed;
            continue;
        }
        if section == "[rust.dependencies]" && !trimmed.starts_with('#') {
            dependencies.push_str(line);
            dependencies.push('\n');
        } else if section == "[rust]"
            && let Some(value) = trimmed.strip_prefix("cargo-authority")
            && let Some(value) = value.trim_start().strip_prefix('=')
        {
            authority = Some(root.join(value.trim().trim_matches('"')));
        }
    }
    if dependencies.trim().is_empty() {
        return None;
    }
    let mut manifest = format!(
        "[package]\nname = {name:?}\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[dependencies]\n{dependencies}"
    );
    if let Some(authority) = &authority
        && let Ok(text) = std::fs::read_to_string(authority)
    {
        let patches = rawr_patches(&text, authority.parent().unwrap_or(root));
        if !patches.trim().is_empty() {
            manifest.push('\n');
            manifest.push_str(patches.trim());
            manifest.push('\n');
        }
    }
    Some((manifest, authority))
}

/// Rawr's own `[patch.*]` tables from its manifest `text`, paths absolute
/// against its `root`.
fn rawr_patches(text: &str, root: &std::path::Path) -> String {
    let mut patches = String::new();
    let mut inside = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            inside = trimmed.starts_with("[patch");
        }
        if inside && !trimmed.starts_with('#') {
            patches.push_str(line);
            patches.push('\n');
        }
    }
    let patches = as_rust_manifest(&patches, root);
    // `as_rust_manifest` closes with its own `[workspace]`; the copy has one.
    patches
        .trim_end()
        .trim_end_matches("[workspace]")
        .to_owned()
}

/// `manifest` with the facade's `iced` features (from its manifest
/// `facade`) added to the app's own inline `iced` table, as Cargo would
/// unify them when Rawr runs the app. Unchanged without both.
#[cfg_attr(not(feature = "native-runner"), allow(dead_code))]
fn with_facade_iced(manifest: &str, facade: &str) -> String {
    let is_iced = |line: &str| {
        line.strip_prefix("iced")
            .is_some_and(|rest| rest.trim_start().starts_with('='))
    };
    let list = |line: &str| {
        let (_, rest) = line.split_once("features")?;
        let (_, rest) = rest.split_once('[')?;
        Some(rest.split_once(']')?.0.to_owned())
    };
    let mut section = "";
    let mut features = None;
    for line in facade.lines() {
        if line.starts_with('[') {
            section = line.trim();
        } else if section == "[dependencies]" && is_iced(line) {
            features = list(line);
            break;
        }
    }
    let Some(features) = features else {
        return manifest.to_owned();
    };
    let mut output = String::with_capacity(manifest.len() + features.len() + 16);
    let mut section = "";
    for line in manifest.lines() {
        if line.starts_with('[') {
            section = line.trim();
        }
        let inline = line.trim_end().ends_with('}');
        if section == "[dependencies]" && is_iced(line) && inline {
            let merged = match line.split_once("features") {
                Some((before, after)) if after.trim_start().starts_with('=') => {
                    let (assign, rest) = after.split_once('[').unwrap_or((after, ""));
                    let separator = if rest.trim_start().starts_with(']') {
                        ""
                    } else {
                        ", "
                    };
                    format!("{before}features{assign}[{features}{separator}{rest}")
                }
                _ => {
                    let body = line.trim_end().trim_end_matches('}').trim_end();
                    format!("{body}, features = [{features}] }}")
                }
            };
            output.push_str(&merged);
        } else {
            output.push_str(line);
        }
        output.push('\n');
    }
    output
}

/// Copy `from` to `to`, renaming `.rawr` files to `.rs` (with previews made
/// inert, `inert_previews`).
fn copy_renamed(from: &std::path::Path, to: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        if entry.file_type()?.is_dir() {
            copy_renamed(&path, &to.join(&name))?;
        } else if path
            .extension()
            .is_some_and(|extension| extension == "rawr")
        {
            let text = std::fs::read_to_string(&path)?;
            std::fs::write(to.join(&name).with_extension("rs"), inert_previews(&text))?;
        } else {
            std::fs::copy(&path, to.join(&name))?;
        }
    }
    Ok(())
}

/// `#[preview(..)]` is Rawr's attribute, unknown to Rust. In the
/// `--as-rust` copy it becomes `#[doc = stringify!(..)]`, an inert doc
/// attribute over the same tokens (the parentheses stay balanced as written),
/// and a bare `#[preview]` becomes `#[doc = ""]`. The user's source is unchanged.
fn inert_previews(text: &str) -> String {
    text.replace("#[preview(", "#[doc = stringify!(")
        .replace("#[preview]", "#[doc = \"\"]")
}

/// The app's `Cargo.toml` for the copy under `target/`: relative `path`
/// values outside target tables (`[lib]`, `[[bin]]`, ..) become absolute
/// against `root`, and it is its own workspace.
fn as_rust_manifest(text: &str, root: &std::path::Path) -> String {
    let mut output = String::with_capacity(text.len() + 16);
    let mut target_table = false;
    let mut workspace = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with('[') {
            let header = trimmed
                .trim_start_matches('[')
                .split(']')
                .next()
                .unwrap_or("")
                .trim();
            target_table = matches!(header, "lib" | "bin" | "example" | "test" | "bench");
            workspace |= header == "workspace";
        }
        let mut rest = line;
        if !target_table {
            while let Some(at) = rest.find("path") {
                let (before, after) = rest.split_at(at + 4);
                output.push_str(before);
                rest = after;
                let value = after.trim_start();
                let Some(value) = value.strip_prefix('=').map(str::trim_start) else {
                    continue;
                };
                let Some(quoted) = value.strip_prefix('"') else {
                    continue;
                };
                let Some(end) = quoted.find('"') else {
                    continue;
                };
                let relative = std::path::Path::new(&quoted[..end]);
                if relative.is_absolute() {
                    continue;
                }
                output.push_str(&format!(
                    " = {:?}",
                    root.join(relative).display().to_string()
                ));
                rest = &quoted[end + 1..];
            }
        }
        output.push_str(rest);
        output.push('\n');
    }
    if !workspace {
        output.push_str("\n[workspace]\n");
    }
    output
}

#[cfg(test)]
mod tests {
    #[test]
    fn as_rust_manifest_absolutizes_dependency_paths_only() {
        let text = "[package]\nname = \"app\"\n\n[lib]\npath = \"src/lib.rs\"\n\n[dependencies]\nlocal = { path = \"../local\", version = \"1\" }\nfar = { path = \"/abs/far\" }\n\n[patch.crates-io]\ncosmic-text = { path = \"vendor/cosmic\" }\n";
        let manifest = super::as_rust_manifest(text, std::path::Path::new("/app"));
        assert!(manifest.contains("path = \"src/lib.rs\""), "{manifest}");
        assert!(
            manifest.contains("local = { path = \"/app/../local\", version = \"1\" }"),
            "{manifest}"
        );
        assert!(
            manifest.contains("far = { path = \"/abs/far\" }"),
            "{manifest}"
        );
        assert!(
            manifest.contains("cosmic-text = { path = \"/app/vendor/cosmic\" }"),
            "{manifest}"
        );
        assert!(manifest.ends_with("\n[workspace]\n"), "{manifest}");
    }

    #[test]
    fn the_facade_iced_features_join_the_app_iced() {
        let facade = "[dependencies]\niced = { git = \"g\", rev = \"r\", features = [\"tokio\", \"wgpu\"] }\n";
        let app =
            "[dependencies]\niced = { git = \"g\", rev = \"r\", features = [\"advanced\"] }\n";
        assert_eq!(
            super::with_facade_iced(app, facade),
            "[dependencies]\niced = { git = \"g\", rev = \"r\", features = [\"tokio\", \"wgpu\", \"advanced\"] }\n"
        );
        let bare = "[dependencies]\niced = { git = \"g\" }\n";
        assert_eq!(
            super::with_facade_iced(bare, facade),
            "[dependencies]\niced = { git = \"g\", features = [\"tokio\", \"wgpu\"] }\n"
        );
        let plain = "[dependencies]\nserde = \"1\"\n";
        assert_eq!(super::with_facade_iced(plain, facade), plain);
    }

    #[test]
    fn previews_become_inert_doc_attributes() {
        let text = "#[preview(name = \"A\", size = (64, 48))]\nfn a() {}\n#[preview]\nfn b() {}\n";
        assert_eq!(
            super::inert_previews(text),
            "#[doc = stringify!(name = \"A\", size = (64, 48))]\nfn a() {}\n#[doc = \"\"]\nfn b() {}\n"
        );
    }

    #[test]
    fn a_rawr_toml_rust_project_becomes_a_package_with_its_dependencies_and_patches() {
        let directory = std::env::temp_dir().join(format!("rawr-as-rust-{}", std::process::id()));
        std::fs::create_dir_all(&directory).unwrap();
        std::fs::write(
            directory.join("Cargo.toml"),
            "[workspace]\nmembers = [\"ui\"]\n\n[patch.crates-io]\ncosmic-text = { path = \"vendor/cosmic\" }\n",
        )
        .unwrap();
        let text = "[package]\nname = \"gallery\"\n\n[rust]\ncargo-authority = \"Cargo.toml\"\n\n[rust.dependencies]\nui = { package = \"gallery-ui\", path = \"ui\" }\n\n[rust.exclude]\n\"ui::X\" = \"why\"\n";
        let (manifest, authority) =
            super::rawr_toml_manifest("gallery", text, &directory).unwrap();
        std::fs::remove_dir_all(&directory).unwrap();
        assert_eq!(authority, Some(directory.join("Cargo.toml")));
        let cosmic = directory.join("vendor/cosmic");
        assert_eq!(
            manifest,
            format!(
                "[package]\nname = \"gallery\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[dependencies]\nui = {{ package = \"gallery-ui\", path = \"ui\" }}\n\n\n[patch.crates-io]\ncosmic-text = {{ path = {:?} }}\n",
                cosmic.display().to_string()
            )
        );
        assert!(super::rawr_toml_manifest("plain", "[package]\nname = \"plain\"\n", &directory).is_none());
    }

    #[test]
    fn rawr_patches_keep_only_patch_tables() {
        let text = "[package]\nname = \"rawr\"\n\n[patch.crates-io]\n# a note\nlocal = { path = \"vendor/local\" }\nremote = { git = \"https://x\", rev = \"1\" }\n\n[features]\nstd = []\n";
        let patches = super::rawr_patches(text, std::path::Path::new("/rawr"));
        assert_eq!(patches.trim(), "[patch.crates-io]\nlocal = { path = \"/rawr/vendor/local\" }\nremote = { git = \"https://x\", rev = \"1\" }");
    }
}

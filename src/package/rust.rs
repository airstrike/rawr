//! Opaque transformation from the crate's Cargo dependencies to one importer request.

use std::path::Path;

use rawr_import::cargo::config::{Dependency, Environment, GitReference as Reference};
use rawr_import::cargo::{Limits, Request};
use rawr_import::extract::Policy;

use super::graph::Graph;
use super::manifest::rust::{Config, GitReference, Source};

impl Graph {
    /// The importer limits profile: `Limits::standard()`. Every owner,
    /// resident and fallback route must use this same profile.
    #[must_use]
    pub fn rust_limits(&self) -> Limits {
        Limits::standard()
    }

    /// Transform the crate's Cargo dependencies into one sealed import request.
    pub fn rust_import(
        &self,
        environment: Environment,
        limits: &Limits,
    ) -> rawr_import::Result<Option<Request>> {
        let entry = self.entry_package();
        let Some(config) = entry.rust.as_ref() else {
            return Ok(None);
        };
        script_project(entry)?;
        let app = app_crate(self, &environment)?;
        request(entry, config, environment, None, app, &[], limits).map(Some)
    }

    /// The Rust imports, resolved in `universe` (the importer's plain one).
    /// `demand` holds the Rust paths Rawr names below wholesale crate
    /// re-exports (see [`super::Error::rust_demand`]). The runner links the
    /// `rawr` crate and the crate's own dependencies only.
    #[doc(hidden)]
    pub fn rust_import_plain(
        &self,
        environment: Environment,
        universe: rawr_import::cargo::universe::Catalog,
        demand: &[Box<str>],
        limits: &Limits,
    ) -> rawr_import::Result<Option<Request>> {
        let entry = self.entry_package();
        let Some(config) = entry.rust.as_ref() else {
            return Ok(None);
        };
        script_project(entry)?;
        let app = app_crate(self, &environment)?;
        request(
            entry,
            config,
            environment,
            Some(universe),
            app,
            demand,
            limits,
        )
        .map(Some)
    }

    /// The request that acquires intrinsic facts (D650), with no demand and
    /// no runner universe. With no `closure` it declares the program's
    /// dependencies, as the probe that finds their library closure; with
    /// one it declares exactly that closure.
    #[doc(hidden)]
    pub fn rust_inventory_plain(
        &self,
        environment: Environment,
        closure: &[Dependency],
        limits: &Limits,
    ) -> rawr_import::Result<Option<Request>> {
        let entry = self.entry_package();
        let Some(config) = entry.rust.as_ref() else {
            return Ok(None);
        };
        script_project(entry)?;
        let app = app_crate(self, &environment)?;
        request_inventory(entry, config, environment, app, closure, limits).map(Some)
    }

    /// The Rust edition of the entry crate's source, `2015` when its Cargo
    /// manifest names none, as Cargo reads it.
    #[doc(hidden)]
    #[must_use]
    pub fn rust_edition(&self) -> String {
        self.entry_package()
            .cargo()
            .and_then(|cargo| cargo.edition.clone())
            .unwrap_or_else(|| "2015".to_owned())
    }
}

impl Graph {
    /// Whether `alias` names a crate the Rust import selects: a Cargo
    /// dependency of the entry package, its own crate or its binary's.
    #[doc(hidden)]
    #[must_use]
    pub fn rust_declares(&self, alias: &str) -> bool {
        let entry = self.entry_package();
        let Some(config) = entry.rust.as_ref() else {
            return false;
        };
        config
            .dependencies
            .iter()
            .any(|dependency| dependency.alias.as_str() == alias)
            || entry.rust_crate() == Some(alias)
            || (entry.bin().is_some() && alias == super::island::BIN)
    }

    /// The Rust paths the Rawr source names, found by a cheap
    /// syntactic pass (`use` items and qualified paths, with `use` bindings
    /// expanded). Only paths below a Cargo dependency alias or the package's
    /// own crate are kept, and a path that only
    /// qualifies a longer one is dropped: demanding the longer path extracts
    /// every item on the way. Sorted and deduplicated, so the same source
    /// gives the same demand. An empty result when the crate imports no Rust.
    #[doc(hidden)]
    #[must_use]
    pub fn rust_demand_seed(&self) -> Vec<Box<str>> {
        if self.entry_package().rust.is_none() {
            return Vec::new();
        }
        let entry = self.entry_package();
        let app = entry.rust_crate();
        let declared = |path: &[&str]| path.len() >= 2 && self.rust_declares(path[0]);
        let mut paths = Vec::new();
        for source in self.sources() {
            for path in seed::paths(source.text()) {
                let segments = match app {
                    Some(app) if source.package() == entry.name() => {
                        app_crate_path(entry, app, source.modules(), &path)
                            .unwrap_or_else(|| path.iter().map(String::as_str).collect())
                    }
                    _ => path.iter().map(String::as_str).collect::<Vec<_>>(),
                };
                if declared(&segments) {
                    paths.push(segments.join("::"));
                }
            }
        }
        // An async `main`'s entry, whose registration the compiler writes
        // (D513.4).
        if let (Some(app), Some(_)) = (app, entry.entry()) {
            paths.push(format!("{app}::{}::start", super::entry::MODULE));
        }
        // Each marked static native code holds, which marked code reads
        // where it is (#1291).
        if let Some(app) = app {
            for path in self.native_statics() {
                paths.push(
                    std::iter::once(app)
                        .chain(path.iter().map(crate::ImmutableString::as_str))
                        .collect::<Vec<_>>()
                        .join("::"),
                );
            }
        }
        // Each type of the crate's own an instance native code calls a
        // marked generic at names, which the resolver reads (D869).
        if let Some(app) = app {
            for instance in self.native_instances() {
                for ty in &instance.types {
                    paths.extend(crate_paths(ty).map(|path| format!("{app}::{path}")));
                }
            }
        }
        paths.sort_unstable();
        paths.dedup();
        let qualifies = |short: &str, long: &str| {
            long.strip_prefix(short)
                .is_some_and(|rest| rest.starts_with("::"))
        };
        paths
            .iter()
            .filter(|path| !paths.iter().any(|long| qualifies(path, long)))
            .map(|path| Box::from(path.as_str()))
            .collect()
    }
}

impl Graph {
    /// The method names the Rawr source calls (`value.name(…)`), found by
    /// the same syntactic pass as [`Self::rust_demand_seed`], sorted and
    /// deduplicated. When resolution misses a member of an imported type,
    /// these are demanded on that type at once, so a builder chain costs one
    /// more extraction rather than one for each method it calls.
    #[doc(hidden)]
    #[must_use]
    pub fn rust_method_names(&self) -> Vec<Box<str>> {
        let mut names = self
            .sources()
            .iter()
            .flat_map(|source| seed::methods(source.text()).into_iter().map(Box::from))
            .collect::<Vec<Box<str>>>();
        names.sort_unstable();
        names.dedup();
        names
    }
}

/// The paths below `crate::` that `ty`, a type rustc wrote from the
/// crate's root, names: `grid::Message` of `Option<crate::grid::Message>`.
fn crate_paths(ty: &str) -> impl Iterator<Item = String> + '_ {
    ty.split(|character: char| {
        !(character.is_alphanumeric() || character == '_' || character == ':')
    })
    .filter_map(|path| path.strip_prefix("crate::"))
    .filter(|path| !path.is_empty())
    .map(str::to_owned)
}

/// A script path (`crate::model::Thing`, `super::model::Thing`, or
/// `model::Thing` from the module declaring `model`) that lands in a `.rs`
/// module, spelled through the app's crate name (`app::model::Thing`).
fn app_crate_path<'a>(
    entry: &super::graph::Package,
    app: &'a str,
    modules: &'a [crate::ImmutableString],
    path: &'a [String],
) -> Option<Vec<&'a str>> {
    let first = path.first()?;
    let supers = path
        .iter()
        .take_while(|segment| *segment == "super")
        .count();
    let mut absolute = Vec::new();
    let rest = if first == "crate" {
        &path[1..]
    } else if supers > 0 {
        absolute.extend(
            modules[..modules.len().checked_sub(supers)?]
                .iter()
                .map(|module| module.as_str()),
        );
        &path[supers..]
    } else {
        absolute.extend(modules.iter().map(|module| module.as_str()));
        path
    };
    absolute.extend(rest.iter().map(String::as_str));
    entry
        .is_native_path(&absolute)
        .then(|| std::iter::once(app).chain(absolute).collect())
}

/// A script's expanded manifest and its lockfile, in its directory in
/// rawr's area: the project Cargo builds the script's runner in (D608
/// point 6).
fn script_project(entry: &super::graph::Package) -> rawr_import::Result<()> {
    match entry.cargo() {
        Some(cargo) if entry.is_script() => {
            super::manifest::script::materialize(cargo).map_err(|reason| {
                rawr_import::Error::Request {
                    reason: reason.into_boxed_str(),
                }
            })
        }
        _ => Ok(()),
    }
}

/// The app's own crate as a Cargo path dependency. Rawr writes a
/// generated package under `crate/` in the project's Rawr directory
/// (`target/rawr/crate/` for a crate of its own): the app's `[package]` and
/// `[dependencies]`, a copy of each `.rs` module under `target/rawr/crate/src/`
/// with crate-wide visibility made public (`polyglot::widened`), and a
/// library root that declares each `.rs` module a marked module declares, in
/// the same tree (`#[path]` on one private inline module rooted at the
/// copies), re-exported at the crate root. Rust follows the `.rs` modules'
/// own `mod` declarations. The marked modules are absent, so the crate
/// builds without them. Files are rewritten only when they change.
///
/// A crate whose root is native (D501) is laid out as Cargo has it: its
/// root and native modules copied under `target/rawr/crate/src/`, the root
/// with the island's runtime support at its end, and in place of each
/// marked file its generated trampolines (`island::shadow`). A save that
/// changes no trampoline rewrites nothing, so nothing is built again.
///
/// A program that runs from the package's library has a native binary
/// beside it (#1107): the library is laid out with trampolines, as a
/// native root's crate is, and the binary gets a generated package of its
/// own under `bin/`, a library named [`island::BIN`] whose root is a copy
/// of `src/main.rs` with the island's runtime support at its end. It
/// depends on the library by the library's crate name, as Cargo builds the
/// binary, so the binary reaches the marked modules through the library.
///
/// [`island::BIN`]: super::island::BIN
/// The generated packages of the program's own crate, as [`app_crate`]
/// lays them out.
struct App {
    /// Each package, as a path dependency of the request.
    crates: Vec<Dependency>,
    /// The directories of those whose check records the native instances
    /// of marked generics (D866): a native root's crate and its binary's.
    recorded: Vec<std::path::PathBuf>,
}

fn app_crate(graph: &Graph, environment: &Environment) -> rawr_import::Result<App> {
    let entry = graph.entry_package();
    let (Some(alias), Some(cargo)) = (entry.rust_crate(), entry.cargo()) else {
        if let Some(module) = entry.native_root().or_else(|| entry.rust_modules().first()) {
            return Err(rawr_import::Error::Request {
                reason: format!(
                    "`{}` is a Rust module, so the project root needs a Cargo.toml with a `[package]`",
                    module.path().display()
                )
                .into_boxed_str(),
            });
        }
        return Ok(App {
            crates: Vec::new(),
            recorded: Vec::new(),
        });
    };
    let module_root = graph
        .entry_source()
        .path()
        .parent()
        .unwrap_or(entry.root())
        .to_path_buf();
    let directory = rawr_import::location::rawr_directory(entry.root(), environment).join("crate");
    let package = cargo.path.parent().unwrap_or(entry.root());
    let copies = directory.join("src");
    let mut instances = Vec::new();
    let mut files = if let Some(root) = entry.native_root() {
        let shadow = graph
            .island_shadow()
            .unwrap_or_else(|| super::island::Shadow {
                files: Vec::new(),
                shape: super::island::Shape::default(),
                refused: Vec::new(),
                objects: Vec::new(),
                generic: std::collections::BTreeMap::new(),
            });
        if !shadow.refused.is_empty() {
            return Err(rawr_import::Error::Request {
                reason: shadow.refused.join("\n").into_boxed_str(),
            });
        }
        let relative = root
            .path()
            .strip_prefix(&module_root)
            .unwrap_or(root.path());
        instances = graph.instance_entries(&shadow.generic);
        let mut files = vec![
            (
                directory.join("Cargo.toml"),
                app_manifest(cargo, &Path::new("src").join(relative)),
            ),
            (
                copies.join(relative),
                native_root(
                    root,
                    package,
                    !entry.is_library(),
                    &shadow.objects,
                    &instances,
                ),
            ),
        ];
        files.extend(
            shadow
                .files
                .into_iter()
                .map(|(path, text)| (copies.join(path), text)),
        );
        files
    } else {
        let mut library = app_library(entry, &copies);
        if let Some(attribute) = entry.entry() {
            library.push_str(&super::entry::source(attribute));
        }
        vec![
            (
                directory.join("Cargo.toml"),
                app_manifest(cargo, Path::new("lib.rs")),
            ),
            (directory.join("lib.rs"), library),
        ]
    };
    let named = named_by_instances(&instances);
    for module in entry.rust_modules() {
        let Ok(relative) = module.path().strip_prefix(&module_root) else {
            continue;
        };
        let declared_by_rawr = !declared_by_rust(entry, module.modules());
        let widened =
            super::polyglot::widened(module.text(), declared_by_rawr, module.path(), package);
        // A native module on the way to a marked one is the marked one's
        // ancestor, whose every item rustc lets it name, as the root's
        // (#1467).
        let copy = if entry.is_bridge(module.modules()) {
            super::polyglot::root_items_public(&widened)
        } else if let Some(names) = named.get(
            &module
                .modules()
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        ) {
            super::polyglot::named_items_public(&widened, names)
        } else {
            widened
        };
        files.push((copies.join(relative), copy));
    }
    if !entry.is_script() {
        files.extend(build_script(cargo, &directory, entry.features())?);
    }
    write_changed(files)?;
    if entry.native_root().is_some() || !entry.rust_modules().is_empty() {
        link_assets(&module_root, &copies)?;
    }
    // The features Cargo enabled for the package under the command's
    // feature flags, and no others: `default` among them when it is on.
    let features = || entry.features().names();
    let mut crates = vec![Dependency::path(alias, &directory)?
        .package(&cargo.package)?
        .features(features(), false)?];
    let mut recorded = Vec::new();
    if entry.native_root().is_some() || entry.bin().is_some() {
        recorded.push(directory.clone());
    }
    if let Some(bin) = entry.bin() {
        crates.push(
            bin_crate(bin, cargo, &directory, &module_root, entry.features())?
                .features(features(), false)?,
        );
        recorded.push(directory.with_file_name("bin"));
    }
    Ok(App { crates, recorded })
}

/// What the support table names below the crate's root, by the module
/// that declares it: each module on the way to an item its instances'
/// types and entries name, and the item itself (D869). The root's own
/// items are its descendants' to name, the table's included.
fn named_by_instances(
    instances: &[super::instances::Entry],
) -> std::collections::BTreeMap<Vec<String>, std::collections::BTreeSet<String>> {
    let mut named = std::collections::BTreeMap::<_, std::collections::BTreeSet<_>>::new();
    let texts = instances.iter().flat_map(|instance| {
        instance
            .types
            .iter()
            .chain(std::iter::once(&instance.function))
    });
    for text in texts {
        for path in crate_paths(text) {
            let segments = path.split("::").map(str::to_owned).collect::<Vec<_>>();
            for end in 1..segments.len() {
                named
                    .entry(segments[..end].to_vec())
                    .or_default()
                    .insert(segments[end].clone());
            }
        }
    }
    named
}

/// The generated package of a native binary that runs the program from the
/// library in `library`: [`app_crate`] says how it is laid out. Its build
/// script's `cfg`s read `features`, the package's.
fn bin_crate(
    bin: &super::graph::Bin,
    cargo: &super::manifest::cargo::Cargo,
    library: &Path,
    module_root: &Path,
    features: &crate::project::cargo::features::Enabled,
) -> rawr_import::Result<Dependency> {
    let directory = library.with_file_name("bin");
    let package = cargo.path.parent().unwrap_or(module_root);
    let copies = directory.join("src");
    let mut files = vec![
        (directory.join("Cargo.toml"), bin_manifest(cargo, library)),
        (
            copies.join("main.rs"),
            native_root(bin.root(), package, true, &[], &[]),
        ),
    ];
    // The binary's modules are beside its root, wherever Cargo finds it.
    let module_root = bin.root().path().parent().unwrap_or(module_root);
    for module in bin.modules() {
        let Ok(relative) = module.path().strip_prefix(module_root) else {
            continue;
        };
        files.push((
            copies.join(relative),
            super::polyglot::widened(module.text(), false, module.path(), package),
        ));
    }
    files.extend(build_script(cargo, &directory, features)?);
    write_changed(files)?;
    Dependency::path(super::island::BIN, &directory)?.package(&bin_package(cargo))
}

/// Where in a generated package the copy of the build script `script`
/// lives: [`BUILD_DIRECTORY`] holds it and its modules.
fn build_copy(script: &Path) -> Option<std::path::PathBuf> {
    script
        .file_name()
        .map(|name| Path::new(BUILD_DIRECTORY).join(name))
}

/// The directory of a generated package that holds the build script's copy
/// and the copies of its modules, laid out as they are beside the script.
const BUILD_DIRECTORY: &str = "build-script";

/// The package's build script copied into `directory`, the generated
/// package standing in for it (#1136), at [`build_copy`], with the modules
/// it declares copied beside it as they are beside the script, so a `mod`
/// resolves as it does for Cargo (#1143). A module's file is found by
/// Rust's rules (`name.rs` or `name/mod.rs` under its parent's directory);
/// one under `#[path]` is read from where that path names. Paths through
/// a source file and `CARGO_MANIFEST_DIR` are absolute
/// (`polyglot::widened`), and the script's `main` runs as
/// [`BUILD_SCRIPT`] has it: in the package's own directory, with
/// `CARGO_MANIFEST_DIR` naming it, so what it reads at run time is the
/// package's, and with its relative `rerun-if-changed` paths made
/// absolute, so Cargo watches the package's files and runs it again
/// exactly when Cargo's build of the package would. A module a `cfg`
/// leaves out under `features`, the package's on the host Cargo builds
/// the script for, is not copied.
fn build_script(
    cargo: &super::manifest::cargo::Cargo,
    directory: &Path,
    features: &crate::project::cargo::features::Enabled,
) -> rawr_import::Result<Vec<(std::path::PathBuf, String)>> {
    let (Some(script), Some(copy)) = (
        cargo.build.as_deref(),
        cargo.build.as_deref().and_then(build_copy),
    ) else {
        return Ok(Vec::new());
    };
    let package = cargo.path.parent().unwrap_or(directory);
    let read = |path: &Path| {
        std::fs::read_to_string(path).map_err(|source| rawr_import::Error::Io {
            operation: "read build script",
            path: path.to_path_buf(),
            source,
        })
    };
    let original = read(script)?;
    let text = super::polyglot::widened(&original, false, script, package);
    let text = match super::polyglot::main_renamed(&text, "__rawr_package_main") {
        Some(renamed) => format!(
            "{renamed}\n{}",
            BUILD_SCRIPT.replace(
                "\"{package}\"",
                &format!("{:?}", package.display().to_string())
            )
        ),
        None => text,
    };
    let mut files = vec![(directory.join(&copy), text)];
    let here = script.parent().unwrap_or(package);
    let there = directory.join(BUILD_DIRECTORY);
    let mut pending = vec![(Vec::<String>::new(), original)];
    while let Some((modules, text)) = pending.pop() {
        for declaration in super::polyglot::declarations(&text, features) {
            if declaration.explicit_path || declaration.disabled {
                continue;
            }
            let mut relative = modules.iter().collect::<std::path::PathBuf>();
            relative.extend(&declaration.path);
            let found = [relative.with_extension("rs"), relative.join("mod.rs")]
                .into_iter()
                .filter(|relative| here.join(relative).is_file())
                .collect::<Vec<_>>();
            // With neither file or both, rustc reports the copy's
            // declaration as it would the script's.
            let [file] = found.as_slice() else {
                continue;
            };
            let path = here.join(file);
            let text = read(&path)?;
            files.push((
                there.join(file),
                super::polyglot::widened(&text, false, &path, package),
            ));
            let mut child = modules.clone();
            child.extend(declaration.path);
            pending.push((child, text));
        }
    }
    Ok(files)
}

/// The `main` of a build script's copy: it runs the package's own `main`
/// (renamed `__rawr_package_main`) again as a child, in the package's
/// directory `"{package}"`, and passes on what it prints with each relative
/// `rerun-if-changed` path made absolute against that directory.
const BUILD_SCRIPT: &str = r#"
// Generated by Rawr: the package's build script runs in its own directory.
fn main() -> ::std::process::ExitCode {
    use ::std::io::{BufRead as _, Write as _};
    const PACKAGE: &str = "{package}";
    const CHILD: &str = "__RAWR_PACKAGE_BUILD_SCRIPT";
    if ::std::env::var_os(CHILD).is_some() {
        return ::std::process::Termination::report(__rawr_package_main());
    }
    let mut child = ::std::process::Command::new(::std::env::current_exe().unwrap())
        .current_dir(PACKAGE)
        .env("CARGO_MANIFEST_DIR", PACKAGE)
        .env(CHILD, "1")
        .stdout(::std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let output = ::std::io::BufReader::new(child.stdout.take().unwrap());
    let mut stdout = ::std::io::stdout().lock();
    for line in output.split(b'\n') {
        let line = line.unwrap();
        let text = ::std::str::from_utf8(&line).ok();
        let watched = text.and_then(|text| {
            ["cargo::rerun-if-changed=", "cargo:rerun-if-changed="]
                .into_iter()
                .find_map(|key| Some((key, text.strip_prefix(key)?)))
        });
        match watched {
            Some((key, path)) if ::std::path::Path::new(path).is_relative() => {
                let path = ::std::path::Path::new(PACKAGE).join(path);
                writeln!(stdout, "{key}{}", path.display()).unwrap();
            }
            _ => {
                stdout.write_all(&line).unwrap();
                stdout.write_all(b"\n").unwrap();
            }
        }
    }
    let status = child.wait().unwrap();
    ::std::process::ExitCode::from(status.code().map_or(1, |code| code as u8))
}
"#;

/// The package name of a binary's generated package.
fn bin_package(cargo: &super::manifest::cargo::Cargo) -> String {
    format!("{}-rawr-bin", cargo.package)
}

/// Write each file whose text differs from what it holds, so a file that
/// stays the same is not built again.
/// Each file of the module tree at `from` that isn't Rust source, at its
/// place under `to` beside the copied modules, so a macro that reads one
/// relative to its module reads what the original reads: a crate's macro
/// that expands to `include_str!("shader/vert.wgsl")` in the copy, as
/// iced's integration example's `wgpu::include_wgsl!` does. A file is
/// linked where it can be and copied otherwise, and kept while it is as
/// new as the original. Hidden directories, `target` and the copies' own
/// tree are passed over.
fn link_assets(from: &Path, to: &Path) -> rawr_import::Result<()> {
    let io = |operation, path: &Path| {
        let path = path.to_path_buf();
        move |source| rawr_import::Error::Io {
            operation,
            path,
            source,
        }
    };
    let mut pending = vec![from.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = std::fs::read_dir(&directory).map_err(io("read directory", &directory))?;
        for entry in entries {
            let entry = entry.map_err(io("read directory", &directory))?;
            let path = entry.path();
            let name = entry.file_name();
            let kind = entry.file_type().map_err(io("read file type", &path))?;
            if kind.is_dir() {
                let passed = name.to_string_lossy().starts_with('.')
                    || name == "target"
                    || to.starts_with(&path);
                if !passed {
                    pending.push(path);
                }
                continue;
            }
            if !kind.is_file() || path.extension().is_some_and(|extension| extension == "rs") {
                continue;
            }
            let Ok(relative) = path.strip_prefix(from) else {
                continue;
            };
            let target = to.join(relative);
            let original = entry.metadata().map_err(io("read metadata", &path))?;
            let current = std::fs::metadata(&target).ok().is_some_and(|linked| {
                linked.len() == original.len()
                    && linked
                        .modified()
                        .ok()
                        .zip(original.modified().ok())
                        .is_some_and(|(linked, original)| linked >= original)
            });
            if current {
                continue;
            }
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(io("create directory", parent))?;
            }
            let _ = std::fs::remove_file(&target);
            if std::fs::hard_link(&path, &target).is_err() {
                std::fs::copy(&path, &target).map_err(io("copy", &target))?;
            }
        }
    }
    Ok(())
}

fn write_changed(files: Vec<(std::path::PathBuf, String)>) -> rawr_import::Result<()> {
    let io = |operation, path: &Path| {
        let path = path.to_path_buf();
        move |source| rawr_import::Error::Io {
            operation,
            path,
            source,
        }
    };
    for (path, text) in files {
        if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(io("create directory", parent))?;
            }
            std::fs::write(&path, text).map_err(io("write", &path))?;
        }
    }
    Ok(())
}

/// A native crate root as the shadow crate's root: its own items public
/// (`polyglot::root_items_public`), so the runner reaches every native
/// module and every root item a marked module may name, `#[path]` and
/// `include_str!` paths absolute, and the island's runtime support at the
/// end. A `forbid(unsafe_code)` becomes `deny`, which the support module's
/// own `allow` can lift for its two calls into the runner. A binary's
/// root (`main`) has the `main` the support runs, and `objects` are the
/// makers of the crate's stand-ins (`island::Shadow::objects`).
fn native_root(
    root: &super::graph::RustModule,
    package: &Path,
    main: bool,
    objects: &[(String, String)],
    instances: &[super::instances::Entry],
) -> String {
    let text = super::polyglot::widened(root.text(), false, root.path(), package);
    let public = super::polyglot::root_items_public(&text);
    let public = public.replace("#![forbid(unsafe_code)]", "#![deny(unsafe_code)]");
    format!(
        "{public}\n{}",
        super::island::support(main, objects, instances)
    )
}

pub(super) fn app_manifest(cargo: &super::manifest::cargo::Cargo, library: &Path) -> String {
    let mut text = manifest_head(
        cargo,
        "the crate without its marked modules",
        &cargo.package,
        &cargo.crate_name,
        library,
    );
    text.push_str(&dependency_table(cargo));
    text
}

/// A native binary's generated manifest: a library of the binary's root,
/// with the package's dependencies and the package's own library in
/// `library`, by its crate name.
fn bin_manifest(cargo: &super::manifest::cargo::Cargo, library: &Path) -> String {
    let mut text = manifest_head(
        cargo,
        "the binary, which runs the program from its library",
        &bin_package(cargo),
        super::island::BIN,
        Path::new("src/main.rs"),
    );
    // The library's features are the ones the runner gives it.
    text.push_str(&format!(
        "{} = {{ package = {:?}, path = {:?}, default-features = false }}\n",
        cargo.crate_name,
        cargo.package,
        library.display().to_string()
    ));
    text.push_str(&dependency_table(cargo));
    text
}

/// A generated manifest up to its `[dependencies]` header: `package`, a
/// library named `library` at `path`, and `cargo`'s version, edition and
/// build script. The library is the package's one target, so a library
/// at `src/main.rs` is not Cargo's binary too (#1135).
fn manifest_head(
    cargo: &super::manifest::cargo::Cargo,
    what: &str,
    package: &str,
    library: &str,
    path: &Path,
) -> String {
    use std::fmt::Write as _;
    let mut text = format!(
        "# Generated by Rawr from {:?}: {what}.\n\
         [package]\nname = {package:?}\nversion = {:?}\n",
        cargo.path.display().to_string(),
        cargo.version.as_deref().unwrap_or("0.0.0"),
    );
    if let Some(edition) = &cargo.edition {
        let _ = writeln!(text, "edition = {edition:?}");
    }
    // The build script's copy ([`build_script`]), if the package has one.
    match cargo.build.as_deref().and_then(build_copy) {
        Some(copy) => {
            let _ = writeln!(text, "build = {:?}", copy.display().to_string());
        }
        None => text.push_str("build = false\n"),
    }
    // Two packages may not link one native library, so only the package's
    // own library says it does.
    if package == cargo.package
        && let Some(links) = &cargo.links
    {
        let _ = writeln!(text, "links = {links}");
    }
    let _ = write!(
        text,
        "autobins = false\npublish = false\n\n[lib]\nname = {library:?}\npath = {:?}\n\n[dependencies]\n",
        path.display().to_string(),
    );
    text
}

/// `cargo`'s dependencies as `[dependencies]` lines, its
/// `[build-dependencies]`, its `[features]` (#1144), then the empty
/// `[workspace]` that keeps a generated package out of any other. A
/// generated package is a path dependency with the features Cargo enabled
/// for the package and no default ones, so it builds with the features
/// `cargo build` gives the package under the command's feature flags.
fn dependency_table(cargo: &super::manifest::cargo::Cargo) -> String {
    let mut text = dependency_lines(&cargo.dependencies);
    if !cargo.build_dependencies.is_empty() {
        text.push_str("\n[build-dependencies]\n");
        text.push_str(&dependency_lines(&cargo.build_dependencies));
    }
    let declared = cargo
        .dependencies
        .iter()
        .chain(&cargo.build_dependencies)
        .collect::<Vec<_>>();
    text.push_str(&cargo.features.manifest(&declared));
    text.push_str("\n[workspace]\n");
    text
}

/// One `key = { .. }` line per dependency, by the key the package's
/// manifest declares it by, which its features name.
fn dependency_lines(dependencies: &[super::manifest::rust::Dependency]) -> String {
    use std::fmt::Write as _;
    let mut text = String::new();
    for dependency in dependencies {
        let mut fields = Vec::new();
        if let Some(package) = &dependency.package {
            fields.push(format!("package = {package:?}"));
        }
        match &dependency.source {
            Source::Path(path) => fields.push(format!("path = {:?}", path.display().to_string())),
            Source::Registry { version } => fields.push(format!("version = {version:?}")),
            Source::Git {
                url,
                reference,
                version,
            } => {
                fields.push(format!("git = {url:?}"));
                if let Some((key, name)) = reference.key_value() {
                    fields.push(format!("{key} = {name:?}"));
                }
                if let Some(version) = version {
                    fields.push(format!("version = {version:?}"));
                }
            }
            // Only a runner's snapshot keeps an unresolved inheritance.
            Source::Workspace => fields.push("workspace = true".to_owned()),
        }
        if !dependency.features.is_empty() {
            fields.push(format!("features = {:?}", dependency.features));
        }
        if !dependency.default_features {
            fields.push("default-features = false".to_owned());
        }
        if dependency.optional {
            fields.push("optional = true".to_owned());
        }
        let _ = writeln!(text, "{} = {{ {} }}", dependency.key, fields.join(", "));
    }
    text
}

/// A `.rs` module some other `.rs` module declares: a proper prefix of its
/// path is itself a `.rs` module, and not one on the way to a marked module
/// (#1467), whose children's `pub(super)` items marked code sees as rustc
/// lets the marked module's ancestors' descendants see them.
fn declared_by_rust(entry: &super::graph::Package, path: &[crate::ImmutableString]) -> bool {
    (1..path.len()).any(|length| {
        let parent = &path[..length];
        !entry.is_bridge(parent)
            && entry
                .rust_modules()
                .iter()
                .any(|other| other.modules() == parent)
    })
}

fn app_library(entry: &super::graph::Package, module_root: &std::path::Path) -> String {
    use std::collections::BTreeMap;
    // The native modules a marked module declares.
    #[derive(Default)]
    struct Node {
        file: bool,
        children: BTreeMap<String, Node>,
    }
    let mut root = Node::default();
    for module in entry.rust_modules() {
        let path = module.modules();
        if declared_by_rust(entry, path) {
            continue;
        }
        let mut node = &mut root;
        for segment in path {
            node = node.children.entry(segment.to_string()).or_default();
        }
        node.file = true;
    }
    fn render(node: &Node, depth: usize, text: &mut String) {
        for (name, child) in &node.children {
            let indent = "    ".repeat(depth);
            if child.file {
                text.push_str(&format!("{indent}pub mod {name};\n"));
            } else {
                text.push_str(&format!("{indent}pub mod {name} {{\n"));
                render(child, depth + 1, text);
                text.push_str(&format!("{indent}}}\n"));
            }
        }
    }
    let mut text = format!(
        "// Generated by Rawr: the .rs modules of {:?} in their module tree.\n\
         #[path = {:?}]\nmod __rawr_src {{\n",
        entry.name(),
        module_root.display().to_string(),
    );
    render(&root, 1, &mut text);
    text.push_str("}\n");
    // A package with no `.rs` modules re-exports none.
    let names = root.children.keys().cloned().collect::<Vec<_>>();
    if !names.is_empty() {
        text.push_str(&format!("pub use __rawr_src::{{{}}};\n", names.join(", ")));
    }
    text
}

/// A syntactic scan of Rawr source for the Rust paths it names. It needs no
/// parse: comments and string literals are skipped, `use` trees bind local
/// names, and every `a::b` sequence is a path. Imprecision only widens or
/// narrows demand; resolution still decides, and a miss is demanded later.
mod seed {
    #[derive(Clone, Copy, Eq, PartialEq)]
    enum Token<'a> {
        Ident(&'a str),
        Path,
        Star,
        Open,
        Close,
        Comma,
        End,
        Dot,
        Call,
        Other,
    }

    fn tokens(text: &str) -> Vec<Token<'_>> {
        let bytes = text.as_bytes();
        let mut tokens = Vec::new();
        let mut index = 0;
        while index < bytes.len() {
            let byte = bytes[index];
            match byte {
                b'/' if bytes.get(index + 1) == Some(&b'/') => {
                    while index < bytes.len() && bytes[index] != b'\n' {
                        index += 1;
                    }
                }
                b'/' if bytes.get(index + 1) == Some(&b'*') => {
                    let mut depth = 0_usize;
                    while index < bytes.len() {
                        if bytes[index..].starts_with(b"/*") {
                            depth += 1;
                            index += 2;
                        } else if bytes[index..].starts_with(b"*/") {
                            depth -= 1;
                            index += 2;
                            if depth == 0 {
                                break;
                            }
                        } else {
                            index += 1;
                        }
                    }
                }
                b'"' | b'`' => {
                    index += 1;
                    while index < bytes.len() && bytes[index] != byte {
                        index += if bytes[index] == b'\\' { 2 } else { 1 };
                    }
                    index += 1;
                    tokens.push(Token::Other);
                }
                b'\'' => {
                    // A character literal; nothing else in Rawr opens with `'`.
                    index += 1;
                    if bytes.get(index) == Some(&b'\\') {
                        index += 1;
                    }
                    while index < bytes.len() && bytes[index] != b'\'' && bytes[index] != b'\n' {
                        index += 1;
                    }
                    index += 1;
                    tokens.push(Token::Other);
                }
                b':' if bytes.get(index + 1) == Some(&b':') => {
                    tokens.push(Token::Path);
                    index += 2;
                }
                b'{' => {
                    tokens.push(Token::Open);
                    index += 1;
                }
                b'*' => {
                    tokens.push(Token::Star);
                    index += 1;
                }
                b'}' => {
                    tokens.push(Token::Close);
                    index += 1;
                }
                b',' => {
                    tokens.push(Token::Comma);
                    index += 1;
                }
                b';' => {
                    tokens.push(Token::End);
                    index += 1;
                }
                b'.' => {
                    tokens.push(Token::Dot);
                    index += 1;
                }
                b'(' => {
                    tokens.push(Token::Call);
                    index += 1;
                }
                _ if byte == b'_' || byte.is_ascii_alphabetic() => {
                    let start = index;
                    while index < bytes.len()
                        && (bytes[index] == b'_' || bytes[index].is_ascii_alphanumeric())
                    {
                        index += 1;
                    }
                    tokens.push(Token::Ident(&text[start..index]));
                }
                _ if byte.is_ascii_whitespace() => index += 1,
                _ => {
                    // Digits run with their suffixes (`1u8`) as one token.
                    if byte.is_ascii_digit() {
                        while index < bytes.len()
                            && (bytes[index] == b'_' || bytes[index].is_ascii_alphanumeric())
                        {
                            index += 1;
                        }
                    } else {
                        index += 1;
                    }
                    tokens.push(Token::Other);
                }
            }
        }
        tokens
    }

    /// One `a::b::c` run starting at `index`, and the index after it.
    fn path<'a>(tokens: &[Token<'a>], mut index: usize) -> (Vec<&'a str>, usize) {
        let mut segments = Vec::new();
        while let Some(Token::Ident(name)) = tokens.get(index) {
            segments.push(*name);
            index += 1;
            if tokens.get(index) == Some(&Token::Path)
                && matches!(tokens.get(index + 1), Some(Token::Ident(_)))
            {
                index += 1;
            } else {
                break;
            }
        }
        (segments, index)
    }

    /// Every path the text names, `use` bindings expanded.
    pub(super) fn paths(text: &str) -> Vec<Vec<String>> {
        let tokens = tokens(text);
        let mut bindings: Vec<(&str, Vec<&str>)> = Vec::new();
        let mut qualified: Vec<Vec<&str>> = Vec::new();
        // `mod download;`: a path `download::Progress` names the module,
        // even beside `use download::download;`, a function of the same
        // name in the value namespace.
        let mut modules: Vec<&str> = Vec::new();
        let mut index = 0;
        while index < tokens.len() {
            match tokens[index] {
                Token::Ident("mod") => {
                    if let Some(Token::Ident(name)) = tokens.get(index + 1) {
                        modules.push(name);
                    }
                    index += 2;
                }
                // `extern crate serde as _serde;`: `_serde` names the crate,
                // as a derive's expansion writes it.
                Token::Ident("extern")
                    if tokens.get(index + 1) == Some(&Token::Ident("crate"))
                        && let Some(Token::Ident(name)) = tokens.get(index + 2) =>
                {
                    let mut local = *name;
                    index += 3;
                    if tokens.get(index) == Some(&Token::Ident("as"))
                        && let Some(Token::Ident(alias)) = tokens.get(index + 1)
                    {
                        local = alias;
                        index += 2;
                    }
                    if local != *name {
                        bindings.push((local, vec![*name]));
                    }
                }
                Token::Ident("use") => {
                    let (prefix, next) = path(&tokens, index + 1);
                    index = next;
                    if prefix.is_empty() {
                        continue;
                    }
                    if tokens.get(index) == Some(&Token::Path)
                        && tokens.get(index + 1) == Some(&Token::Star)
                    {
                        // A glob demands every public name of the module.
                        let mut glob = prefix;
                        glob.push("*");
                        qualified.push(glob);
                        index += 2;
                    } else if tokens.get(index) == Some(&Token::Path)
                        && tokens.get(index + 1) == Some(&Token::Open)
                    {
                        index += 2;
                        while let Some(Token::Ident(name)) = tokens.get(index) {
                            let mut full = prefix.clone();
                            let mut local = *name;
                            if *name == "self" {
                                local = prefix[prefix.len() - 1];
                            } else {
                                full.push(name);
                            }
                            index += 1;
                            if tokens.get(index) == Some(&Token::Ident("as"))
                                && let Some(Token::Ident(alias)) = tokens.get(index + 1)
                            {
                                local = alias;
                                index += 2;
                            }
                            bindings.push((local, full));
                            if tokens.get(index) == Some(&Token::Comma) {
                                index += 1;
                            }
                        }
                    } else {
                        let mut local = prefix[prefix.len() - 1];
                        if tokens.get(index) == Some(&Token::Ident("as"))
                            && let Some(Token::Ident(alias)) = tokens.get(index + 1)
                        {
                            local = alias;
                            index += 2;
                        }
                        bindings.push((local, prefix));
                    }
                }
                Token::Ident(_) => {
                    let (segments, next) = path(&tokens, index);
                    index = next;
                    if segments.len() >= 2 {
                        qualified.push(segments);
                    }
                }
                _ => index += 1,
            }
        }
        // A `use` path starts at any name in scope, as rustc resolves it:
        // `use keyboard::key;` beside `use iced::keyboard;`, at module level
        // or in a block, names `iced::keyboard::key`.
        let bindings = bindings
            .iter()
            .map(|(local, full)| (*local, through_bindings(full, &bindings, &modules)))
            .collect::<Vec<_>>();
        let mut paths = bindings
            .iter()
            .map(|(_, full)| full.iter().map(|segment| (*segment).to_owned()).collect())
            .collect::<Vec<Vec<String>>>();
        // A name two `use`s bind, in Rust's two namespaces, as `badge` of
        // `use m::badge::{self, badge};`: a path through it goes through
        // the module, which a syntactic pass can't tell from the function,
        // so it demands the path through each.
        for segments in qualified {
            let mut starts = bindings
                .iter()
                .filter(|(local, _)| *local == segments[0] && !modules.contains(local))
                .map(|(_, full)| full.clone())
                .collect::<Vec<_>>();
            if starts.is_empty() {
                starts.push(vec![segments[0]]);
            }
            for mut full in starts {
                full.extend_from_slice(&segments[1..]);
                paths.push(full.into_iter().map(str::to_owned).collect());
            }
        }
        paths
    }

    /// `full` with its first segment, while another `use` binds it, written
    /// as that binding's path: `iced::keyboard::key` for `keyboard::key`
    /// after `use iced::keyboard;`. A `mod` of that name is the module, and a
    /// binding's own name (`use serde::serde;`) is the crate's.
    fn through_bindings<'a>(
        full: &[&'a str],
        bindings: &[(&'a str, Vec<&'a str>)],
        modules: &[&'a str],
    ) -> Vec<&'a str> {
        let mut full = full.to_vec();
        // Each step expands through a binding not taken yet, so a cycle of
        // imports ends.
        let mut taken = vec![false; bindings.len()];
        while full.len() >= 2
            && !modules.contains(&full[0])
            && let Some(index) = (0..bindings.len()).rev().find(|&index| {
                let (local, path) = &bindings[index];
                !taken[index] && *local == full[0] && path.as_slice() != full.as_slice()
            })
        {
            taken[index] = true;
            let path = &bindings[index].1;
            if path[0] == full[0] {
                break;
            }
            full.splice(..1, path.iter().copied());
        }
        full
    }

    /// Every method the text calls by name (`.name(` or `.name::<`).
    pub(super) fn methods(text: &str) -> Vec<&str> {
        tokens(text)
            .windows(3)
            .filter_map(|window| match window {
                [Token::Dot, Token::Ident(name), Token::Call | Token::Path] => Some(*name),
                _ => None,
            })
            .collect()
    }

    #[cfg(test)]
    mod tests {
        #[test]
        fn seed_names_the_methods_a_chain_calls() {
            let text =
                "fn app() { rect().width(Size::fill()).on_press(move |_| *count.write() += 1)\n\
                .child(format!(\"x.y(\", n.read())); v.iter().collect::<Vec<_>>(); s.len }\n";
            assert_eq!(
                super::methods(text),
                ["width", "on_press", "write", "child", "read", "iter", "collect"]
            );
        }

        fn joined(text: &str) -> Vec<String> {
            let mut paths = super::paths(text)
                .into_iter()
                .map(|path| path.join("::"))
                .collect::<Vec<_>>();
            paths.sort();
            paths.dedup();
            paths
        }

        #[test]
        fn seed_keeps_a_module_path_beside_a_function_of_its_name() {
            let text = "mod download;\nuse download::download;\n\
                enum Update { Downloading(download::Progress) }\n";
            assert_eq!(joined(text), ["download::Progress", "download::download"]);
        }

        /// iced's todos: a block's `use keyboard::key;` starts at the
        /// module's `use iced::keyboard;`, and `key::Named::Tab` through
        /// both (#1044).
        #[test]
        fn seed_starts_a_use_at_another_use_binding() {
            let text = "use iced::keyboard;\n\
                fn subscription() { use keyboard::key; use key::{Named}; \
                let _ = key::Named::Tab; Named::Enter; }\n";
            assert_eq!(
                joined(text),
                [
                    "iced::keyboard",
                    "iced::keyboard::key",
                    "iced::keyboard::key::Named",
                    "iced::keyboard::key::Named::Enter",
                    "iced::keyboard::key::Named::Tab",
                ]
            );
        }

        #[test]
        fn seed_expands_use_bindings_and_skips_comments_and_strings() {
            let text = "use iced::widget::{self, button as b, Column};\n\
                use iced::Element;\n\
                use iced::advanced::*;\n\
                // iced::commented::Out\n\
                /* iced::block::Out */\n\
                fn view() -> Element { let s = \"iced::quoted::Out\"; \
                widget::text::Style::default(); b(1u8); Column::new(); iced::Color::BLACK }\n";
            assert_eq!(
                joined(text),
                [
                    "iced::Color::BLACK",
                    "iced::Element",
                    "iced::advanced::*",
                    "iced::widget",
                    "iced::widget::Column",
                    "iced::widget::Column::new",
                    "iced::widget::button",
                    "iced::widget::text::Style::default",
                ]
            );
        }
    }
}

fn request_inventory(
    entry: &super::graph::Package,
    config: &Config,
    environment: Environment,
    app: App,
    closure: &[Dependency],
    limits: &Limits,
) -> rawr_import::Result<Request> {
    let mut builder = Request::builder(
        entry.root(),
        &entry.root().join("target/rawr/cache"),
        limits,
    )?
    .environment(environment)
    .profile(&config.profile);
    builder = builder.cargo_authority(&config.cargo_authority);
    // The probe declares what the program does; the inventory declares
    // its whole library closure, each crate by its own name.
    if closure.is_empty() {
        for dependency in dependencies(config)? {
            builder = builder.dependency(dependency);
        }
        for app in app.crates {
            builder = builder.dependency(app);
        }
    } else {
        for dependency in closure {
            builder = builder.dependency(dependency.clone());
        }
    }
    builder
        .inventory()
        .policy(Policy::manifest().seal())
        .build()
}

fn dependencies(config: &Config) -> rawr_import::Result<Vec<Dependency>> {
    let mut dependencies = Vec::new();
    for configured in &config.dependencies {
        let mut dependency = match &configured.source {
            Source::Path(path) => Dependency::path(&configured.alias, path)?,
            Source::Registry { version } => Dependency::registry(&configured.alias, version)?,
            Source::Git {
                url,
                reference,
                version,
            } => {
                let reference = match reference {
                    GitReference::Branch(name) => Reference::Branch(name.as_str().into()),
                    GitReference::Tag(name) => Reference::Tag(name.as_str().into()),
                    GitReference::Rev(name) => Reference::Rev(name.as_str().into()),
                    GitReference::DefaultBranch => Reference::DefaultBranch,
                };
                let dependency = Dependency::git(&configured.alias, url, reference)?;
                match version {
                    Some(version) => dependency.git_version(version),
                    None => dependency,
                }
            }
            Source::Workspace => {
                return Err(rawr_import::Error::Request {
                    reason: format!(
                        "Rust dependency `{}` inherits from a workspace that Cargo did not resolve",
                        configured.alias
                    )
                    .into_boxed_str(),
                });
            }
        };
        if let Some(package) = &configured.package {
            dependency = dependency.package(package)?;
        }
        dependency = dependency.features(&configured.features, configured.default_features)?;
        dependencies.push(dependency);
    }
    Ok(dependencies)
}

fn request(
    entry: &super::graph::Package,
    config: &Config,
    environment: Environment,
    universe: Option<rawr_import::cargo::universe::Catalog>,
    app: App,
    demand: &[Box<str>],
    limits: &Limits,
) -> rawr_import::Result<Request> {
    let mut builder = Request::builder(
        entry.root(),
        &entry.root().join("target/rawr/cache"),
        limits,
    )?
    .environment(environment)
    .profile(&config.profile);
    builder = builder.cargo_authority(&config.cargo_authority);
    for dependency in dependencies(config)? {
        builder = builder.dependency(dependency);
    }
    for app in app.crates {
        builder = builder.dependency(app);
    }
    for directory in &app.recorded {
        builder = builder.record(directory);
    }
    if let Some(universe) = universe {
        builder = builder.resolution_universe(universe);
    }
    for path in demand {
        builder = builder.demand(path.split("::"));
    }
    builder.policy(Policy::manifest().seal()).build()
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use crate::package::manifest::cargo::Cargo;

    fn cargo(path: &Path, text: &str) -> Cargo {
        Cargo::parse(
            path.to_path_buf(),
            text.to_owned(),
            path.parent().unwrap(),
            false,
        )
        .unwrap()
        .unwrap()
    }

    /// A generated manifest declares each dependency by the package's own
    /// key, an optional one optional, and the package's `[features]` as
    /// written, less a value naming the attribute crate it leaves out
    /// (#1144). A binary's has them too.
    #[test]
    fn a_generated_manifest_has_the_package_s_features_and_optional_dependencies() {
        let cargo = cargo(
            Path::new("/app/Cargo.toml"),
            r#"
[package]
name = "app"
version = "0.1.0"

[dependencies]
tracing-subscriber = { version = "0.3", optional = true }
rawr-macros = { version = "0.3", optional = true }

[features]
default = ["logs"]
logs = ["dep:tracing-subscriber", "tracing-subscriber/fmt"]
preview = ["dep:rawr-macros"]
"#,
        );
        let expected = "tracing-subscriber = { package = \"tracing-subscriber\", \
            version = \"0.3\", optional = true }\n\n[features]\n\
            \"default\" = [\"logs\"]\n\
            \"logs\" = [\"dep:tracing-subscriber\", \"tracing-subscriber/fmt\"]\n\
            \"preview\" = []\n\n[workspace]\n";
        let library = super::app_manifest(&cargo, Path::new("lib.rs"));
        assert!(library.ends_with(expected), "{library}");
        let bin = super::bin_manifest(&cargo, Path::new("/app/target/rawr/crate"));
        assert!(bin.ends_with(expected), "{bin}");
        assert!(library.contains("build = false\n"), "{library}");
    }

    /// A build script's `mod` declarations resolve beside the script, as
    /// for Cargo: its copy has each module's copy beside it, and a
    /// module's own modules under its directory (#1143).
    #[test]
    fn a_build_script_s_modules_are_copied_beside_its_copy() {
        let root = std::env::temp_dir().join(format!("rawr-build-modules-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let package = root.join("app");
        let write = |path: &str, text: &str| {
            let path = package.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        write(
            "tools/build.rs",
            "mod icons;\n#[cfg(test)]\nmod tests;\nfn main() {}\n",
        );
        write("tools/icons.rs", "mod names;\nmod list { mod inner; }\n");
        write(
            "tools/icons/names.rs",
            "pub const N: &str = include_str!(\"n.txt\");\n",
        );
        write("tools/icons/list/inner.rs", "\n");
        let cargo = cargo(
            &package.join("Cargo.toml"),
            "[package]\nname = \"app\"\nversion = \"0.1.0\"\nbuild = \"tools/build.rs\"\n",
        );
        let directory = root.join("generated");
        let mut files = super::build_script(
            &cargo,
            &directory,
            &crate::project::cargo::features::Enabled::default(),
        )
        .unwrap();
        files.sort();
        let copies = files
            .iter()
            .map(|(path, _)| path.strip_prefix(&directory).unwrap().to_path_buf())
            .collect::<Vec<_>>();
        assert_eq!(
            copies,
            [
                "build-script/build.rs",
                "build-script/icons/list/inner.rs",
                "build-script/icons/names.rs",
                "build-script/icons.rs",
            ]
            .map(PathBuf::from)
        );
        assert!(
            files[2].1.contains(&format!(
                "include_str!(\"{}\")",
                package.join("tools/icons/n.txt").display()
            )),
            "{}",
            files[2].1
        );
        let generated = super::app_manifest(&cargo, Path::new("lib.rs"));
        assert!(
            generated.contains("build = \"build-script/build.rs\"\n"),
            "{generated}"
        );
        std::fs::remove_dir_all(&root).unwrap();
    }

    /// A module tree's other files are beside the copied modules, as a
    /// crate macro's `include_str!` in a copy reads them (#314), and an
    /// edited one is taken again; Rust sources, hidden directories and
    /// `target` are not.
    #[test]
    fn a_module_tree_s_other_files_are_beside_the_copies() {
        let root = std::env::temp_dir().join(format!("rawr-assets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let source = root.join("src");
        let write = |path: &str, text: &str| {
            let path = source.join(path);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, text).unwrap();
        };
        write("scene.rs", "fn f() {}\n");
        write("shader/vert.wgsl", "vertex\n");
        write(".git/HEAD", "ref\n");
        write("target/out.txt", "built\n");
        let copies = root.join("generated/src");
        super::link_assets(&source, &copies).unwrap();
        let read = |path: &str| std::fs::read_to_string(copies.join(path)).ok();
        assert_eq!(read("shader/vert.wgsl").as_deref(), Some("vertex\n"));
        assert_eq!(read("scene.rs"), None);
        assert_eq!(read(".git/HEAD"), None);
        assert_eq!(read("target/out.txt"), None);
        std::fs::remove_file(source.join("shader/vert.wgsl")).unwrap();
        write("shader/vert.wgsl", "edited vertex\n");
        super::link_assets(&source, &copies).unwrap();
        assert_eq!(read("shader/vert.wgsl").as_deref(), Some("edited vertex\n"));
        std::fs::remove_dir_all(&root).unwrap();
    }
}

//! Opaque transformation from the crate's Cargo dependencies to one importer request.

use std::path::Path;

use rawr_import::cargo::config::{Dependency, Environment};
use rawr_import::cargo::{Limits, Request};
use rawr_import::extract::Policy;

use super::graph::Graph;
use super::manifest::rust::{Config, Source};

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
        let app = app_crate(self, &environment, limits)?;
        request(entry, config, environment, app, limits).map(Some)
    }

    /// The Rust imports with no host profile, resolved in `universe` (the
    /// importer's plain one). `demand` holds the Rust paths Rawr names below
    /// wholesale crate re-exports (see [`super::Error::rust_demand`]). The
    /// runner links the `rawr` crate and the crate's own dependencies only.
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
        let app = app_crate(self, &environment, limits)?;
        request_with_profile(
            entry,
            config,
            environment,
            None,
            Some(universe),
            None,
            app,
            demand,
            limits,
        )
        .map(Some)
    }

    /// Transform the Rust imports with one host-selected exact framework-profile package.
    /// `demand` holds Rust paths Rawr names below wholesale crate re-exports
    /// (see [`super::Error::rust_demand`]). `framework` is the crate the
    /// profile reads, with its alias: it joins the request when the crate
    /// declares no dependency of that alias.
    #[doc(hidden)]
    pub fn rust_import_with_profile(
        &self,
        environment: Environment,
        profile: Dependency,
        framework: Option<(&str, Dependency)>,
        universe: rawr_import::cargo::universe::Catalog,
        demand: &[Box<str>],
        limits: &Limits,
    ) -> rawr_import::Result<Option<Request>> {
        let entry = self.entry_package();
        let Some(config) = entry.rust.as_ref() else {
            return Ok(None);
        };
        let app = app_crate(self, &environment, limits)?;
        request_with_profile(
            entry,
            config,
            environment,
            Some(profile),
            Some(universe),
            framework,
            app,
            demand,
            limits,
        )
        .map(Some)
    }
}

impl Graph {
    /// The Rust paths the Rawr source names, found by a cheap
    /// syntactic pass (`use` items and qualified paths, with `use` bindings
    /// expanded), plus `required` paths a host profile needs. Only paths below
    /// a Cargo dependency alias, or below `framework` (the alias of
    /// the crate the host adds for its profile), are kept, and a path that only
    /// qualifies a longer one is dropped: demanding the longer path extracts
    /// every item on the way. Sorted and deduplicated, so the same source
    /// gives the same demand. An empty result when the crate imports no Rust.
    #[doc(hidden)]
    #[must_use]
    pub fn rust_demand_seed(&self, required: &[&str], framework: Option<&str>) -> Vec<Box<str>> {
        let Some(config) = self.entry_package().rust.as_ref() else {
            return Vec::new();
        };
        let entry = self.entry_package();
        let app = entry.rust_crate();
        let declared = |path: &[&str]| {
            path.len() >= 2
                && (config
                    .dependencies
                    .iter()
                    .any(|dependency| dependency.alias.as_str() == path[0])
                    || app == Some(path[0])
                    || framework == Some(path[0]))
        };
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
        for path in required {
            if declared(&path.split("::").collect::<Vec<_>>()) {
                paths.push((*path).to_owned());
            }
        }
        // An async `main`'s entry, whose registration the compiler writes
        // (D513.4).
        if let (Some(app), Some(_)) = (app, entry.entry()) {
            paths.push(format!("{app}::{}::start", super::entry::MODULE));
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
fn app_crate(
    graph: &Graph,
    environment: &Environment,
    limits: &Limits,
) -> rawr_import::Result<Option<Dependency>> {
    let entry = graph.entry_package();
    let (Some(alias), Some(cargo)) = (entry.rust_crate(), entry.cargo()) else {
        if let Some(module) = entry.native_root().or_else(|| entry.rust_modules().first()) {
            return Err(rawr_import::Error::Request {
                field: "app crate",
                reason: format!(
                    "`{}` is a Rust module, so the project root needs a Cargo.toml with a `[package]`",
                    module.path().display()
                )
                .into_boxed_str(),
            });
        }
        return Ok(None);
    };
    let module_root = graph
        .entry_source()
        .path()
        .parent()
        .unwrap_or(entry.root())
        .to_path_buf();
    let directory = rawr_import::location::rawr_directory(entry.root(), environment).join("crate");
    let copies = directory.join("src");
    let mut files = match entry.native_root() {
        Some(root) => {
            let shadow = graph
                .island_shadow()
                .unwrap_or_else(|| super::island::Shadow {
                    files: Vec::new(),
                    shape: super::island::Shape::default(),
                    refused: Vec::new(),
                });
            if !shadow.refused.is_empty() {
                return Err(rawr_import::Error::Request {
                    field: "native islands",
                    reason: shadow.refused.join("\n").into_boxed_str(),
                });
            }
            let relative = root
                .path()
                .strip_prefix(&module_root)
                .unwrap_or(root.path());
            let mut files = vec![
                (
                    directory.join("Cargo.toml"),
                    app_manifest(cargo, &Path::new("src").join(relative)),
                ),
                (copies.join(relative), native_root(root)),
            ];
            files.extend(
                shadow
                    .files
                    .into_iter()
                    .map(|(path, text)| (copies.join(path), text)),
            );
            files
        }
        None => {
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
        }
    };
    for module in entry.rust_modules() {
        let Ok(relative) = module.path().strip_prefix(&module_root) else {
            continue;
        };
        let declared_by_rawr = !declared_by_rust(entry, module.modules());
        files.push((
            copies.join(relative),
            super::polyglot::widened(module.text(), declared_by_rawr, module.path()),
        ));
    }
    let io = |error: std::io::Error| rawr_import::Error::Request {
        field: "generated app crate",
        reason: error.to_string().into_boxed_str(),
    };
    for (path, text) in files {
        if std::fs::read_to_string(&path).ok().as_deref() != Some(text.as_str()) {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).map_err(io)?;
            }
            std::fs::write(&path, text).map_err(io)?;
        }
    }
    Dependency::path(alias, &directory, limits)?
        .package(&cargo.package)
        .map(Some)
}

/// A native crate root as the shadow crate's root: its `mod` declarations
/// public, so the runner reaches every native module, `#[path]` and
/// `include_str!` paths absolute, and the island's runtime support at the
/// end. A `forbid(unsafe_code)` becomes `deny`, which the support module's
/// own `allow` can lift for its two calls into the runner.
fn native_root(root: &super::graph::RustModule) -> String {
    let text = super::polyglot::widened(root.text(), false, root.path());
    let mut public = String::with_capacity(text.len() + 64);
    let mut cursor = 0;
    for declaration in super::polyglot::declarations(&text) {
        if declaration.path.len() != 1 {
            continue;
        }
        let before = text[..declaration.range.start].trim_end();
        let Some(keyword) = before.strip_suffix("mod") else {
            continue;
        };
        let visibility = keyword.trim_end();
        if visibility.ends_with("pub") || visibility.ends_with(')') {
            continue;
        }
        public.push_str(&text[cursor..keyword.len()]);
        public.push_str("pub ");
        cursor = keyword.len();
    }
    public.push_str(&text[cursor..]);
    let public = public.replace("#![forbid(unsafe_code)]", "#![deny(unsafe_code)]");
    let main = root
        .path()
        .file_name()
        .is_some_and(|name| name == "main.rs");
    format!("{public}\n{}", super::island::support(main))
}

fn app_manifest(cargo: &super::manifest::cargo::Cargo, library: &Path) -> String {
    use std::fmt::Write as _;
    let mut text = format!(
        "# Generated by Rawr from {:?}: the crate without its marked modules.\n\
         [package]\nname = {:?}\nversion = {:?}\n",
        cargo.path.display().to_string(),
        cargo.package,
        cargo.version.as_deref().unwrap_or("0.0.0"),
    );
    if let Some(edition) = &cargo.edition {
        let _ = writeln!(text, "edition = {edition:?}");
    }
    let _ = write!(
        text,
        "publish = false\n\n[lib]\nname = {:?}\npath = {:?}\n\n[dependencies]\n",
        cargo.crate_name,
        library.display().to_string(),
    );
    for dependency in &cargo.dependencies {
        let mut fields = Vec::new();
        if let Some(package) = &dependency.package {
            fields.push(format!("package = {package:?}"));
        }
        match &dependency.source {
            Source::Path(path) => fields.push(format!("path = {:?}", path.display().to_string())),
            Source::Registry { version } => fields.push(format!("version = {version:?}")),
            Source::Git { url, rev } => {
                fields.push(format!("git = {url:?}"));
                fields.push(format!("rev = {rev:?}"));
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
        let _ = writeln!(text, "{} = {{ {} }}", dependency.alias, fields.join(", "));
    }
    text.push_str("\n[workspace]\n");
    text
}

/// A `.rs` module some other `.rs` module declares: a proper prefix of its
/// path is itself a `.rs` module.
fn declared_by_rust(entry: &super::graph::Package, path: &[crate::ImmutableString]) -> bool {
    (1..path.len()).any(|length| {
        entry
            .rust_modules()
            .iter()
            .any(|other| other.modules() == &path[..length])
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
    let names = root.children.keys().cloned().collect::<Vec<_>>();
    text.push_str(&format!(
        "}}\npub use __rawr_src::{{{}}};\n",
        names.join(", ")
    ));
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
        let mut paths = bindings
            .iter()
            .map(|(_, full)| full.iter().map(|segment| (*segment).to_owned()).collect())
            .collect::<Vec<Vec<String>>>();
        for segments in qualified {
            let mut full = bindings
                .iter()
                .rev()
                .find(|(local, _)| *local == segments[0] && !modules.contains(local))
                .map_or_else(|| vec![segments[0]], |(_, full)| full.clone());
            full.extend_from_slice(&segments[1..]);
            paths.push(full.into_iter().map(str::to_owned).collect());
        }
        paths
    }

    #[cfg(test)]
    mod tests {
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

fn request(
    entry: &super::graph::Package,
    config: &Config,
    environment: Environment,
    app: Option<Dependency>,
    limits: &Limits,
) -> rawr_import::Result<Request> {
    request_with_profile(
        entry,
        config,
        environment,
        None,
        None,
        None,
        app,
        &[],
        limits,
    )
}

fn request_with_profile(
    entry: &super::graph::Package,
    config: &Config,
    environment: Environment,
    profile: Option<Dependency>,
    universe: Option<rawr_import::cargo::universe::Catalog>,
    framework: Option<(&str, Dependency)>,
    app: Option<Dependency>,
    demand: &[Box<str>],
    limits: &Limits,
) -> rawr_import::Result<Request> {
    let mut builder = Request::builder(
        entry.root(),
        &entry.root().join("target/rawr/cache"),
        limits,
    )?
    .environment(environment)
    .profile(&config.profile)?;
    builder = builder.cargo_authority(&config.cargo_authority)?;
    for configured in &config.dependencies {
        let mut dependency = match &configured.source {
            Source::Path(path) => Dependency::path(&configured.alias, path, limits)?,
            Source::Registry { version } => {
                Dependency::registry(&configured.alias, version, limits)?
            }
            Source::Git { url, rev } => Dependency::git(&configured.alias, url, rev, limits)?,
            Source::Workspace => {
                return Err(rawr_import::Error::Request {
                    field: "dependency",
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
        builder = builder.dependency(dependency)?;
    }
    if let Some(app) = app {
        builder = builder.dependency(app)?;
    }
    if let Some((alias, framework)) = framework
        && !config
            .dependencies
            .iter()
            .any(|dependency| dependency.alias.as_str() == alias)
    {
        builder = builder.dependency(framework)?;
    }
    if let Some(profile) = profile {
        builder = builder.dependency(profile)?;
    }
    if let Some(universe) = universe {
        builder = builder.resolution_universe(universe)?;
    }
    for path in demand {
        builder = builder.demand(path.split("::"))?;
    }
    builder.policy(Policy::manifest(limits).seal())?.build()
}

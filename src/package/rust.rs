//! Opaque transformation from validated Rawr package metadata to one importer request.

use rawr_import::cargo::config::{Dependency, Environment};
use rawr_import::cargo::{Limits, Request};
use rawr_import::extract::{ManifestSpan, Policy};

use super::graph::Graph;
use super::manifest::rust::{Config, Exclusion, Source};

impl Graph {
    /// The importer limits profile the entry package configures through
    /// `[rust.limits]`, or `Limits::standard()` when it configures none.
    /// Every owner, resident and fallback route must use this same profile.
    #[must_use]
    pub fn rust_limits(&self) -> Limits {
        self.entry_package()
            .rust
            .as_ref()
            .map_or_else(Limits::standard, |config| config.resolved_limits)
    }

    /// Transform the entry package's validated `[rust]` table into one sealed import request.
    pub fn rust_import(
        &self,
        environment: Environment,
        limits: &Limits,
    ) -> rawr_import::Result<Option<Request>> {
        let entry = self.entry_package();
        let Some(config) = entry.rust.as_ref() else {
            return Ok(None);
        };
        let app = app_crate(self, limits)?;
        request(entry, config, environment, app, limits).map(Some)
    }

    /// Transform `[rust]` with one host-selected exact framework-profile package.
    /// `demand` holds Rust paths Rawr names below wholesale crate re-exports
    /// (see [`super::Error::rust_demand`]).
    #[doc(hidden)]
    pub fn rust_import_with_profile(
        &self,
        environment: Environment,
        profile: Dependency,
        universe: rawr_import::cargo::universe::Catalog,
        demand: &[Box<str>],
        limits: &Limits,
    ) -> rawr_import::Result<Option<Request>> {
        let entry = self.entry_package();
        let Some(config) = entry.rust.as_ref() else {
            return Ok(None);
        };
        let app = app_crate(self, limits)?;
        request_with_profile(
            entry,
            config,
            environment,
            Some((profile, universe)),
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
    /// an entry `[rust]` dependency alias are kept, and a path that only
    /// qualifies a longer one is dropped: demanding the longer path extracts
    /// every item on the way. Sorted and deduplicated, so the same source
    /// gives the same demand. An empty result when there is no `[rust]`.
    #[doc(hidden)]
    #[must_use]
    pub fn rust_demand_seed(&self, required: &[&str]) -> Vec<Box<str>> {
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
                    || app == Some(path[0]))
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
        .rust_modules()
        .iter()
        .any(|module| {
            absolute.len() > module.modules().len()
                && module
                    .modules()
                    .iter()
                    .zip(&absolute)
                    .all(|(left, right)| left.as_str() == *right)
        })
        .then(|| std::iter::once(app).chain(absolute).collect())
}

/// The app's own crate as a Cargo path dependency. Rawr writes a
/// generated package under `.rawr/crate/`: the app's `[package]` and
/// `[dependencies]`, a copy of each `.rs` module under `.rawr/crate/src/`
/// with crate-wide visibility made public (`polyglot::widened`), and a
/// library root that declares each `.rs` module a `.rawr` module declares, in
/// the same tree (`#[path]` on one private inline module rooted at the
/// copies), re-exported at the crate root. Rust follows the `.rs` modules'
/// own `mod` declarations. The `.rawr` modules are absent, so the crate
/// builds without them. Files are rewritten only when they change.
fn app_crate(graph: &Graph, limits: &Limits) -> rawr_import::Result<Option<Dependency>> {
    let entry = graph.entry_package();
    let (Some(alias), Some(cargo)) = (entry.rust_crate(), entry.cargo()) else {
        if let Some(module) = entry.rust_modules().first() {
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
    let directory = entry.root().join(".rawr/crate");
    let copies = directory.join("src");
    let mut files = vec![
        (directory.join("Cargo.toml"), app_manifest(cargo)),
        (directory.join("lib.rs"), app_library(entry, &copies)),
    ];
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

fn app_manifest(cargo: &super::manifest::cargo::Cargo) -> String {
    use std::fmt::Write as _;
    let mut text = format!(
        "# Generated by Rawr from {:?}: the crate without its .rawr modules.\n\
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
        "publish = false\n\n[lib]\nname = {:?}\npath = \"lib.rs\"\n\n[dependencies]\n",
        cargo.crate_name
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
    // The `.rs` modules a `.rawr` module declares.
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
        let mut index = 0;
        while index < tokens.len() {
            match tokens[index] {
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
                .find(|(local, _)| *local == segments[0])
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
    request_with_profile(entry, config, environment, None, app, &[], limits)
}

fn request_with_profile(
    entry: &super::graph::Package,
    config: &Config,
    environment: Environment,
    profile: Option<(Dependency, rawr_import::cargo::universe::Catalog)>,
    app: Option<Dependency>,
    demand: &[Box<str>],
    limits: &Limits,
) -> rawr_import::Result<Request> {
    let mut builder = Request::builder(entry.root(), &entry.root().join(".rawr/cache"), limits)?
        .environment(environment)
        .profile(&config.profile)?;
    if let Some(authority) = &config.cargo_authority {
        builder = builder.cargo_authority(authority)?;
    }
    if let Some(target) = &config.target {
        builder = builder.target(target)?;
    }
    for configured in &config.dependencies {
        let mut dependency = match &configured.source {
            Source::Path(path) => Dependency::path(&configured.alias, path, limits)?,
            Source::Registry { version } => {
                Dependency::registry(&configured.alias, version, limits)?
            }
            Source::Git { url, rev } => Dependency::git(&configured.alias, url, rev, limits)?,
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
    if let Some((profile, universe)) = profile {
        builder = builder.dependency(profile)?;
        builder = builder.resolution_universe(universe)?;
    }
    for path in demand {
        builder = builder.demand(path.split("::"))?;
    }
    let manifest = entry
        .manifest()
        .or_else(|| entry.cargo_manifest().map(|(path, _)| path))
        .ok_or_else(|| rawr_import::Error::Request {
            field: "Rust manifest authority",
            reason: "an entry `[rust]` table requires a canonical rawr.toml or Cargo.toml".into(),
        })?;
    let manifest_file = manifest.to_string_lossy();
    let mut policy = Policy::manifest(limits);
    for exclusion in &config.exclusions {
        policy = policy.exclusion(
            &exclusion.alias,
            exclusion.path.iter().map(|part| part.as_str()),
            &exclusion.reason,
            manifest_span(&manifest_file, exclusion, limits)?,
        )?;
    }
    builder.policy(policy.seal())?.build()
}

fn manifest_span<'manifest>(
    manifest: &'manifest str,
    exclusion: &Exclusion,
    limits: &rawr_import::cargo::Limits,
) -> rawr_import::Result<ManifestSpan<'manifest>> {
    let start = exclusion.path_span.start();
    let end = exclusion.reason_span.end();
    let line_start = start.line().ok_or_else(position_error)?;
    let column_start = start
        .position()
        .ok_or_else(position_error)?
        .saturating_sub(1);
    let line_end = end.line().ok_or_else(position_error)?;
    let column_end = end.position().ok_or_else(position_error)?.saturating_sub(1);
    ManifestSpan::new(
        manifest,
        exclusion.path_bytes.start,
        exclusion.reason_bytes.end,
        line_start,
        column_start,
        line_end,
        column_end,
        limits,
    )
}

fn position_error() -> rawr_import::Error {
    rawr_import::Error::Request {
        field: "Rust exclusion span",
        reason: "source positions are unavailable".into(),
    }
}

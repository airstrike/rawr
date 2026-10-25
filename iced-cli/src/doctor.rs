//! Exhaustive, side-effect-free environment diagnosis.

use std::env;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use rawr::{diagnostic, project};
use rawr_iced::native;

const CHECK_NAMES: [&str; 9] = [
    "versions",
    "renderer",
    "window system",
    "display session",
    "project",
    "source",
    "manifest",
    "assets",
    "extensions",
];

/// Outcome of one environment check.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Status {
    /// The observable requirement is satisfied.
    Ok,
    /// The environment may work, but an actionable uncertainty remains.
    Warn,
    /// A required capability or explicitly selected resource is unavailable.
    Fail,
}

/// One ordered doctor check and its canonical diagnostic, when actionable.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Check {
    name: &'static str,
    status: Status,
    summary: String,
    report: Option<diagnostic::Report>,
}

/// Complete ordered diagnosis of the current Rawr environment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Report {
    checks: Vec<Check>,
}

#[derive(Clone, Debug)]
struct Versions {
    cli: String,
    rawr: String,
    adapter: String,
    iced: String,
    iced_revision: String,
    linked: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)] // Every variant is constructed by its corresponding compilation target.
enum Platform {
    MacOs,
    Windows,
    Linux,
    Other,
}

#[derive(Clone, Debug)]
struct Target {
    root: Option<PathBuf>,
    source: PathBuf,
    manifest: Option<PathBuf>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Component {
    General,
    Source,
    Manifest,
}

#[derive(Clone, Debug)]
struct Failure {
    report: diagnostic::Report,
    component: Component,
    missing: bool,
    root: Option<PathBuf>,
    source: Option<PathBuf>,
    manifest: Option<PathBuf>,
}

#[derive(Clone, Debug)]
enum Discovery {
    Found(Target),
    Failed(Failure),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Directory {
    Missing,
    Readable,
    WrongKind,
    Unreadable(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum TextFile {
    Missing,
    Readable,
    WrongKind,
    Unreadable(String),
    InvalidUtf8,
}

trait Probe {
    fn versions(&self) -> Versions;
    fn renderers(&self) -> Vec<native::Renderer>;
    fn window_systems(&self) -> Vec<native::WindowSystem>;
    fn platform(&self) -> Platform;
    fn variable(&self, name: &str) -> Option<OsString>;
    fn discover(&self, path: Option<&Path>) -> Discovery;
    fn directory(&self, path: &Path) -> Directory;
    fn text_file(&self, path: &Path) -> TextFile;
    fn extension_paths(&self) -> Vec<PathBuf>;
}

struct System {
    build: native::Build,
}

impl Check {
    /// Return the stable check name.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.name
    }

    /// Return the check outcome.
    #[must_use]
    pub const fn status(&self) -> Status {
        self.status
    }

    /// Return the concise outcome summary.
    #[must_use]
    pub fn summary(&self) -> &str {
        &self.summary
    }

    /// Return the canonical environment report for WARN or FAIL.
    #[must_use]
    pub const fn diagnostic(&self) -> Option<&diagnostic::Report> {
        self.report.as_ref()
    }

    fn ok(name: &'static str, summary: impl Into<String>) -> Self {
        Self {
            name,
            status: Status::Ok,
            summary: summary.into(),
            report: None,
        }
    }

    fn warning(name: &'static str, report: diagnostic::Report) -> Self {
        debug_assert_eq!(report.category(), diagnostic::Category::Environment);
        debug_assert_eq!(report.severity(), diagnostic::Severity::Warning);
        Self {
            name,
            status: Status::Warn,
            summary: report.message().to_owned(),
            report: Some(report),
        }
    }

    fn failure(name: &'static str, report: diagnostic::Report) -> Self {
        debug_assert_eq!(report.category(), diagnostic::Category::Environment);
        debug_assert_eq!(report.severity(), diagnostic::Severity::Error);
        Self {
            name,
            status: Status::Fail,
            summary: report.message().to_owned(),
            report: Some(report),
        }
    }

    fn skipped(name: &'static str, reason: &str) -> Self {
        Self::warning(
            name,
            diagnostic::Report::warning(
                diagnostic::code::ENVIRONMENT_DOCTOR,
                format!("{name} check skipped: {reason}"),
            )
            .help("resolve the project check, then run `rawr doctor` again")
            .finish(),
        )
    }
}

impl Report {
    /// Run every doctor check against the real process and filesystem.
    #[must_use]
    pub fn examine(path: Option<PathBuf>) -> Self {
        examine_with(
            &System {
                build: native::Build::current(),
            },
            path,
        )
    }

    /// Return checks in their stable execution and display order.
    #[must_use]
    pub fn checks(&self) -> &[Check] {
        &self.checks
    }

    /// Return true only when at least one required check failed.
    #[must_use]
    pub fn has_failures(&self) -> bool {
        self.checks.iter().any(|check| check.status == Status::Fail)
    }
}

impl fmt::Display for Status {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Ok => "OK",
            Self::Warn => "WARN",
            Self::Fail => "FAIL",
        })
    }
}

impl fmt::Display for Report {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        for check in &self.checks {
            writeln!(
                formatter,
                "[{}] {}: {}",
                check.status, check.name, check.summary
            )?;
            if let Some(report) = &check.report {
                writeln!(formatter, "       code: {}", report.code())?;
                if let Some(help) = report.help() {
                    writeln!(formatter, "       help: {help}")?;
                }
                for note in report.notes() {
                    writeln!(formatter, "       note: {note}")?;
                }
            }
        }
        Ok(())
    }
}

impl Probe for System {
    fn versions(&self) -> Versions {
        Versions {
            cli: env!("CARGO_PKG_VERSION").to_owned(),
            rawr: self.build.rawr_version().to_owned(),
            adapter: self.build.adapter_version().to_owned(),
            iced: self.build.iced_version().to_owned(),
            iced_revision: self.build.iced_revision().to_owned(),
            linked: true,
        }
    }

    fn renderers(&self) -> Vec<native::Renderer> {
        self.build.renderers().to_vec()
    }

    fn window_systems(&self) -> Vec<native::WindowSystem> {
        self.build.window_systems().to_vec()
    }

    fn platform(&self) -> Platform {
        #[cfg(target_os = "macos")]
        return Platform::MacOs;
        #[cfg(target_os = "windows")]
        return Platform::Windows;
        #[cfg(target_os = "linux")]
        return Platform::Linux;
        #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
        return Platform::Other;
    }

    fn variable(&self, name: &str) -> Option<OsString> {
        env::var_os(name).filter(|value| !value.is_empty())
    }

    fn discover(&self, path: Option<&Path>) -> Discovery {
        let result = match path {
            Some(path) => project::Target::discover(path),
            None => project::Target::current(),
        };
        match result {
            Ok(target) => Discovery::Found(Target {
                root: target
                    .project()
                    .map(|project| project.root().as_path().to_path_buf()),
                source: target.source().path().to_path_buf(),
                manifest: target
                    .project()
                    .map(|project| project.root().as_path().join("Cargo.toml")),
            }),
            Err(error) => Discovery::Failed(project_failure(&error)),
        }
    }

    fn directory(&self, path: &Path) -> Directory {
        match fs::metadata(path) {
            Ok(metadata) if !metadata.is_dir() => Directory::WrongKind,
            Ok(_) => match fs::read_dir(path) {
                Ok(entries) => {
                    drop(entries);
                    Directory::Readable
                }
                Err(error) => Directory::Unreadable(error.to_string()),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => Directory::Missing,
            Err(error) => Directory::Unreadable(error.to_string()),
        }
    }

    fn text_file(&self, path: &Path) -> TextFile {
        match fs::metadata(path) {
            Ok(metadata) if !metadata.is_file() => TextFile::WrongKind,
            Ok(_) => match fs::read(path) {
                Ok(bytes) if std::str::from_utf8(&bytes).is_ok() => TextFile::Readable,
                Ok(_) => TextFile::InvalidUtf8,
                Err(error) => TextFile::Unreadable(error.to_string()),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => TextFile::Missing,
            Err(error) => TextFile::Unreadable(error.to_string()),
        }
    }

    fn extension_paths(&self) -> Vec<PathBuf> {
        self.variable("RAWR_EXTENSION_PATH")
            .map(|paths| {
                env::split_paths(&paths)
                    .filter(|path| !path.as_os_str().is_empty())
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn examine_with(probe: &impl Probe, path: Option<PathBuf>) -> Report {
    let versions = probe.versions();
    let renderers = probe.renderers();
    let window_systems = probe.window_systems();
    let explicit = path.is_some();
    let discovery = probe.discover(path.as_deref());

    let mut checks = Vec::with_capacity(CHECK_NAMES.len());
    checks.push(check_versions(&versions));
    checks.push(check_renderers(&renderers));
    checks.push(check_window_systems(&window_systems));
    checks.push(check_session(probe, &window_systems));
    checks.push(check_project(&discovery, explicit));
    checks.push(check_source(probe, &discovery));
    checks.push(check_manifest(probe, &discovery));
    checks.push(check_assets(probe, &discovery));
    checks.push(check_extensions(probe));

    debug_assert_eq!(
        checks.iter().map(Check::name).collect::<Vec<_>>(),
        CHECK_NAMES
    );
    Report { checks }
}

fn check_versions(versions: &Versions) -> Check {
    let complete = [
        versions.cli.as_str(),
        versions.rawr.as_str(),
        versions.adapter.as_str(),
        versions.iced.as_str(),
        versions.iced_revision.as_str(),
    ]
    .iter()
    .all(|value| !value.is_empty());
    if versions.linked && complete {
        return Check::ok(
            "versions",
            format!(
                "CLI {}, runtime {}, adapter {}, iced {} @ {}; linked together at build time",
                versions.cli,
                versions.rawr,
                versions.adapter,
                versions.iced,
                short_revision(&versions.iced_revision)
            ),
        );
    }
    Check::failure(
        "versions",
        diagnostic::Report::error(
            diagnostic::code::ENVIRONMENT_DOCTOR,
            "Rawr build version evidence is incomplete or incompatible",
        )
        .help("reinstall the Rawr CLI and native adapter from the same release")
        .finish(),
    )
}

fn check_renderers(renderers: &[native::Renderer]) -> Check {
    if renderers.is_empty() {
        return Check::failure(
            "renderer",
            diagnostic::Report::error(
                diagnostic::code::ENVIRONMENT_DOCTOR,
                "no native iced renderer is compiled into this Rawr build",
            )
            .help("install a Rawr build with a supported native renderer")
            .finish(),
        );
    }
    Check::ok(
        "renderer",
        format!("compiled: {}", join_renderers(renderers)),
    )
}

fn check_window_systems(window_systems: &[native::WindowSystem]) -> Check {
    if window_systems.is_empty() {
        return Check::failure(
            "window system",
            diagnostic::Report::error(
                diagnostic::code::ENVIRONMENT_DOCTOR,
                "no native window system is compiled for this target",
            )
            .help("install a Rawr build for a supported desktop target")
            .finish(),
        );
    }
    Check::ok(
        "window system",
        format!("compiled: {}", join_window_systems(window_systems)),
    )
}

fn check_session(probe: &impl Probe, window_systems: &[native::WindowSystem]) -> Check {
    match probe.platform() {
        Platform::Linux => {
            let wayland = window_systems.contains(&native::WindowSystem::Wayland)
                && probe.variable("WAYLAND_DISPLAY").is_some()
                && probe.variable("XDG_RUNTIME_DIR").is_some();
            let x11 = window_systems.contains(&native::WindowSystem::X11)
                && probe.variable("DISPLAY").is_some();
            if wayland || x11 {
                let session = if wayland && x11 {
                    "Wayland and X11 session variables are present"
                } else if wayland {
                    "Wayland session variables are present"
                } else {
                    "X11 display variable is present"
                };
                Check::ok(
                    "display session",
                    format!("{session}; native launch was not attempted"),
                )
            } else {
                Check::warning(
                    "display session",
                    diagnostic::Report::warning(
                        diagnostic::code::ENVIRONMENT_DOCTOR,
                        "no plausible Linux display session is visible",
                    )
                    .help("start Rawr inside an X11 or Wayland desktop session; for Wayland, set both WAYLAND_DISPLAY and XDG_RUNTIME_DIR")
                    .note("doctor does not open a window, so this check cannot guarantee launch")
                    .finish(),
                )
            }
        }
        Platform::MacOs => remote_session_check(
            probe,
            "macOS desktop session appears plausible; native launch was not attempted",
        ),
        Platform::Windows => {
            if probe.variable("SESSIONNAME").as_deref() == Some(OsStr::new("Services")) {
                Check::warning(
                    "display session",
                    diagnostic::Report::warning(
                        diagnostic::code::ENVIRONMENT_DOCTOR,
                        "Windows service session may not have an interactive desktop",
                    )
                    .help("run Rawr from an interactive Windows desktop session")
                    .note("doctor does not open a window, so this check cannot guarantee launch")
                    .finish(),
                )
            } else {
                remote_session_check(
                    probe,
                    "Windows desktop session appears plausible; native launch was not attempted",
                )
            }
        }
        Platform::Other => Check::warning(
            "display session",
            diagnostic::Report::warning(
                diagnostic::code::ENVIRONMENT_DOCTOR,
                "this target has no side-effect-free display-session plausibility check",
            )
            .help("try `rawr run` from a supported interactive desktop session")
            .note("doctor does not open a window or initialize a compositor")
            .finish(),
        ),
    }
}

fn remote_session_check(probe: &impl Probe, success: &'static str) -> Check {
    if ["CI", "SSH_CONNECTION", "SSH_TTY"]
        .iter()
        .any(|name| probe.variable(name).is_some())
    {
        Check::warning(
            "display session",
            diagnostic::Report::warning(
                diagnostic::code::ENVIRONMENT_DOCTOR,
                "a headless, CI, or remote session may not have desktop access",
            )
            .help("run Rawr from an interactive desktop session")
            .note("doctor does not open a window, so this check cannot guarantee launch")
            .finish(),
        )
    } else {
        Check::ok("display session", success)
    }
}

fn check_project(discovery: &Discovery, explicit: bool) -> Check {
    match discovery {
        Discovery::Found(target) => match &target.root {
            Some(root) => Check::ok("project", format!("canonical root `{}`", root.display())),
            None => Check::ok(
                "project",
                format!("explicit one-file target `{}`", target.source.display()),
            ),
        },
        Discovery::Failed(failure) if failure.missing && !explicit => Check::warning(
            "project",
            diagnostic::Report::warning(
                diagnostic::code::ENVIRONMENT_PROJECT_NOT_FOUND,
                failure.report.message(),
            )
            .help("run inside a project, pass a path, or create one with `rawr new <path> --template iced`")
            .finish(),
        ),
        Discovery::Failed(failure) => Check::failure("project", failure.report.clone()),
    }
}

fn check_source(probe: &impl Probe, discovery: &Discovery) -> Check {
    match discovery {
        Discovery::Found(target) => Check::ok(
            "source",
            format!("readable UTF-8 `{}`", target.source.display()),
        ),
        Discovery::Failed(failure) if failure.component == Component::Source => {
            Check::failure("source", failure.report.clone())
        }
        Discovery::Failed(Failure {
            source: Some(path), ..
        }) => text_file_check(probe, "source", path, false),
        Discovery::Failed(_) => Check::skipped("source", "no canonical source was selected"),
    }
}

fn check_manifest(probe: &impl Probe, discovery: &Discovery) -> Check {
    match discovery {
        Discovery::Found(Target {
            manifest: Some(path),
            ..
        }) => Check::ok("manifest", format!("Cargo manifest `{}`", path.display())),
        Discovery::Found(_) => Check::ok("manifest", "not present; a single `.rs` file needs none"),
        Discovery::Failed(failure) if failure.component == Component::Manifest => {
            Check::failure("manifest", failure.report.clone())
        }
        Discovery::Failed(Failure {
            manifest: Some(path),
            ..
        }) => text_file_check(probe, "manifest", path, true),
        Discovery::Failed(_) => Check::skipped("manifest", "no canonical project was selected"),
    }
}

fn check_assets(probe: &impl Probe, discovery: &Discovery) -> Check {
    let root = match discovery {
        Discovery::Found(target) => target.root.as_ref(),
        Discovery::Failed(failure) => failure.root.as_ref(),
    };
    let Some(root) = root else {
        if matches!(discovery, Discovery::Found(_)) {
            return Check::ok("assets", "not applicable to an explicit one-file target");
        }
        return Check::skipped("assets", "no canonical project root was selected");
    };
    let path = root.join("assets");
    match probe.directory(&path) {
        Directory::Missing => Check::ok("assets", "not present; assets are optional"),
        Directory::Readable => Check::ok(
            "assets",
            format!("optional directory is readable `{}`", path.display()),
        ),
        Directory::WrongKind => Check::failure(
            "assets",
            diagnostic::Report::error(
                diagnostic::code::ENVIRONMENT_DOCTOR,
                format!("assets location `{}` is not a directory", path.display()),
            )
            .help("move assets into an `assets` directory or remove the conflicting file")
            .finish(),
        ),
        Directory::Unreadable(error) => Check::failure(
            "assets",
            diagnostic::Report::error(
                diagnostic::code::ENVIRONMENT_UNREADABLE,
                format!("cannot read assets directory `{}`: {error}", path.display()),
            )
            .help("check directory permissions; an absent assets directory is valid")
            .finish(),
        ),
    }
}

fn check_extensions(probe: &impl Probe) -> Check {
    let paths = probe.extension_paths();
    if paths.is_empty() {
        return Check::ok(
            "extensions",
            "no extension-pack search path configured; extension packs are optional",
        );
    }
    let problems: Vec<_> = paths
        .iter()
        .filter_map(|path| match probe.directory(path) {
            Directory::Readable => None,
            Directory::Missing => Some(format!("`{}` does not exist", path.display())),
            Directory::WrongKind => Some(format!("`{}` is not a directory", path.display())),
            Directory::Unreadable(error) => {
                Some(format!("`{}` is unreadable: {error}", path.display()))
            }
        })
        .collect();
    if problems.is_empty() {
        return Check::ok(
            "extensions",
            format!("{} configured search location(s) are readable", paths.len()),
        );
    }
    let mut builder = diagnostic::Report::warning(
        diagnostic::code::ENVIRONMENT_DOCTOR,
        format!(
            "{} configured extension-pack location(s) need attention",
            problems.len()
        ),
    )
    .help("fix RAWR_EXTENSION_PATH or remove locations that should not be searched");
    for problem in problems {
        builder = builder.note(problem);
    }
    Check::warning("extensions", builder.finish())
}

fn project_failure(error: &project::Error) -> Failure {
    let component = match error {
        project::Error::Unreadable { resource, .. }
        | project::Error::EscapesRoot { resource, .. } => match resource {
            project::Resource::Source => Component::Source,
            project::Resource::Manifest => Component::Manifest,
            project::Resource::Input | project::Resource::Boundary => Component::General,
        },
        project::Error::MissingSource { .. } | project::Error::Unmarked { .. } => Component::Source,
        project::Error::CurrentDirectory(..)
        | project::Error::InputNotFound { .. }
        | project::Error::NotFound { .. }
        | project::Error::Ambiguous { .. }
        | project::Error::UnsupportedInput { .. }
        | project::Error::Legacy { .. } => Component::General,
    };
    let root = match error {
        project::Error::Unreadable { resource, path, .. } => resource_root(*resource, path),
        project::Error::MissingSource { root, .. }
        | project::Error::Unmarked { root, .. }
        | project::Error::EscapesRoot { root, .. } => Some(root.clone()),
        project::Error::CurrentDirectory(..)
        | project::Error::InputNotFound { .. }
        | project::Error::NotFound { .. }
        | project::Error::Ambiguous { .. }
        | project::Error::UnsupportedInput { .. }
        | project::Error::Legacy { .. } => None,
    };
    let source = match error {
        project::Error::Unreadable {
            resource: project::Resource::Source,
            path,
            ..
        }
        | project::Error::EscapesRoot {
            resource: project::Resource::Source,
            path,
            ..
        } => Some(path.clone()),
        project::Error::MissingSource { expected, .. } => Some(expected.clone()),
        _ => root.as_ref().map(|root| root.join("src/main.rs")),
    };
    let manifest = match error {
        project::Error::Unreadable {
            resource: project::Resource::Manifest,
            path,
            ..
        }
        | project::Error::EscapesRoot {
            resource: project::Resource::Manifest,
            path,
            ..
        } => Some(path.clone()),
        _ => root.as_ref().map(|root| root.join("Cargo.toml")),
    };
    Failure {
        report: diagnostic::Report::from_project(error),
        component,
        missing: matches!(error, project::Error::NotFound { .. }),
        root,
        source,
        manifest,
    }
}

fn resource_root(resource: project::Resource, path: &Path) -> Option<PathBuf> {
    match resource {
        project::Resource::Source => path.parent()?.parent().map(Path::to_path_buf),
        project::Resource::Manifest => path.parent().map(Path::to_path_buf),
        project::Resource::Input | project::Resource::Boundary => None,
    }
}

fn text_file_check(probe: &impl Probe, name: &'static str, path: &Path, optional: bool) -> Check {
    match probe.text_file(path) {
        TextFile::Missing if optional => {
            Check::ok(name, "not present; a single `.rs` file needs none")
        }
        TextFile::Missing => Check::failure(
            name,
            diagnostic::Report::error(
                diagnostic::code::ENVIRONMENT_MISSING_SOURCE,
                format!("required Rawr source `{}` does not exist", path.display()),
            )
            .help("create the expected source file or select an explicit `.rs` file")
            .finish(),
        ),
        TextFile::Readable => Check::ok(name, format!("readable UTF-8 `{}`", path.display())),
        TextFile::WrongKind => Check::failure(
            name,
            diagnostic::Report::error(
                diagnostic::code::ENVIRONMENT_UNREADABLE,
                format!("{name} path `{}` is not a regular file", path.display()),
            )
            .help("replace the path with a readable UTF-8 file")
            .finish(),
        ),
        TextFile::Unreadable(error) => Check::failure(
            name,
            diagnostic::Report::error(
                diagnostic::code::ENVIRONMENT_UNREADABLE,
                format!("cannot read {name} `{}`: {error}", path.display()),
            )
            .help("check file permissions and try again")
            .finish(),
        ),
        TextFile::InvalidUtf8 => Check::failure(
            name,
            diagnostic::Report::error(
                diagnostic::code::ENVIRONMENT_UNREADABLE,
                format!("{name} `{}` is not valid UTF-8", path.display()),
            )
            .help("save the file as UTF-8 and try again")
            .finish(),
        ),
    }
}

fn short_revision(revision: &str) -> &str {
    revision.get(..8).unwrap_or(revision)
}

fn join_renderers(renderers: &[native::Renderer]) -> String {
    renderers
        .iter()
        .map(|renderer| match renderer {
            native::Renderer::Wgpu => "wgpu",
            native::Renderer::TinySkia => "tiny-skia",
        })
        .collect::<Vec<_>>()
        .join(", ")
}

fn join_window_systems(window_systems: &[native::WindowSystem]) -> String {
    window_systems
        .iter()
        .map(|system| match system {
            native::WindowSystem::AppKit => "AppKit",
            native::WindowSystem::Windows => "Windows",
            native::WindowSystem::X11 => "X11",
            native::WindowSystem::Wayland => "Wayland",
        })
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;

    struct Fake {
        versions: Versions,
        renderers: Vec<native::Renderer>,
        window_systems: Vec<native::WindowSystem>,
        platform: Platform,
        variables: BTreeMap<String, OsString>,
        discovery: Discovery,
        directories: BTreeMap<PathBuf, Directory>,
        text_files: BTreeMap<PathBuf, TextFile>,
        extensions: Vec<PathBuf>,
    }

    impl Fake {
        fn healthy() -> Self {
            Self {
                versions: Versions {
                    cli: "0.1.0".into(),
                    rawr: "1.25.1".into(),
                    adapter: "0.1.0".into(),
                    iced: "0.15.0-dev".into(),
                    iced_revision: native::ICED_REVISION.into(),
                    linked: true,
                },
                renderers: vec![native::Renderer::Wgpu, native::Renderer::TinySkia],
                window_systems: vec![native::WindowSystem::AppKit],
                platform: Platform::MacOs,
                variables: BTreeMap::new(),
                discovery: Discovery::Found(Target {
                    root: Some(PathBuf::from("/project")),
                    source: PathBuf::from("/project/src/main.rs"),
                    manifest: None,
                }),
                directories: BTreeMap::new(),
                text_files: BTreeMap::new(),
                extensions: Vec::new(),
            }
        }
    }

    impl Probe for Fake {
        fn versions(&self) -> Versions {
            self.versions.clone()
        }

        fn renderers(&self) -> Vec<native::Renderer> {
            self.renderers.clone()
        }

        fn window_systems(&self) -> Vec<native::WindowSystem> {
            self.window_systems.clone()
        }

        fn platform(&self) -> Platform {
            self.platform
        }

        fn variable(&self, name: &str) -> Option<OsString> {
            self.variables.get(name).cloned()
        }

        fn discover(&self, _path: Option<&Path>) -> Discovery {
            self.discovery.clone()
        }

        fn directory(&self, path: &Path) -> Directory {
            self.directories
                .get(path)
                .cloned()
                .unwrap_or(Directory::Missing)
        }

        fn text_file(&self, path: &Path) -> TextFile {
            self.text_files
                .get(path)
                .cloned()
                .unwrap_or(TextFile::Missing)
        }

        fn extension_paths(&self) -> Vec<PathBuf> {
            self.extensions.clone()
        }
    }

    #[test]
    fn every_check_runs_and_two_failures_are_retained_in_stable_order() {
        let mut fake = Fake::healthy();
        fake.versions.linked = false;
        fake.renderers.clear();
        fake.window_systems.clear();

        let report = examine_with(&fake, Some(PathBuf::from("/project")));

        assert!(report.has_failures());
        assert_eq!(
            report.checks().iter().map(Check::name).collect::<Vec<_>>(),
            CHECK_NAMES
        );
        assert_eq!(
            report
                .checks()
                .iter()
                .filter(|check| check.status() == Status::Fail)
                .count(),
            3
        );
        assert!(report.to_string().contains("[FAIL] versions"));
        assert!(report.to_string().contains("[FAIL] renderer"));
        assert!(report.to_string().contains("[FAIL] window system"));
    }

    #[test]
    fn warnings_do_not_fail_and_no_project_does_not_skip_host_checks() {
        let mut fake = Fake::healthy();
        fake.variables.insert("CI".into(), "true".into());
        fake.discovery = missing_project();
        fake.extensions.push(PathBuf::from("/missing/extensions"));

        let report = examine_with(&fake, None);

        assert!(!report.has_failures());
        assert_eq!(report.checks()[0].status(), Status::Ok);
        assert_eq!(report.checks()[1].status(), Status::Ok);
        assert_eq!(report.checks()[2].status(), Status::Ok);
        assert_eq!(report.checks()[3].status(), Status::Warn);
        assert_eq!(report.checks()[4].status(), Status::Warn);
        assert_eq!(report.checks()[8].status(), Status::Warn);
    }

    #[test]
    fn explicit_missing_project_fails_after_all_checks_run() {
        let mut fake = Fake::healthy();
        fake.discovery = missing_project();

        let report = examine_with(&fake, Some(PathBuf::from("/missing")));

        assert!(report.has_failures());
        assert_eq!(report.checks().len(), CHECK_NAMES.len());
        assert_eq!(report.checks()[4].status(), Status::Fail);
    }

    #[test]
    fn source_and_manifest_failures_are_attributed_without_compilation() {
        let mut fake = Fake::healthy();
        fake.discovery = component_failure(Component::Source, "source is unreadable");
        let source = examine_with(&fake, Some(PathBuf::from("/project")));
        assert_eq!(source.checks()[5].status(), Status::Fail);
        assert_eq!(source.checks()[6].status(), Status::Warn);

        fake.discovery = component_failure(Component::Manifest, "manifest is not UTF-8");
        let manifest = examine_with(&fake, Some(PathBuf::from("/project")));
        assert_eq!(manifest.checks()[5].status(), Status::Warn);
        assert_eq!(manifest.checks()[6].status(), Status::Fail);
    }

    #[test]
    fn unreadable_assets_fail_while_absence_is_ok() {
        let mut fake = Fake::healthy();
        let assets = PathBuf::from("/project/assets");
        let absent = examine_with(&fake, Some(PathBuf::from("/project")));
        assert_eq!(absent.checks()[7].status(), Status::Ok);

        fake.directories
            .insert(assets, Directory::Unreadable("permission denied".into()));
        let unreadable = examine_with(&fake, Some(PathBuf::from("/project")));
        assert_eq!(unreadable.checks()[7].status(), Status::Fail);
        assert!(unreadable.checks()[7]
            .summary()
            .contains("permission denied"));
    }

    #[test]
    fn linux_session_variants_are_honest_and_non_fatal() {
        let mut fake = Fake::healthy();
        fake.platform = Platform::Linux;
        fake.window_systems = vec![native::WindowSystem::X11, native::WindowSystem::Wayland];

        let headless = examine_with(&fake, None);
        assert_eq!(headless.checks()[3].status(), Status::Warn);
        assert!(!headless.has_failures());

        fake.variables.insert("DISPLAY".into(), ":0".into());
        let x11 = examine_with(&fake, None);
        assert_eq!(x11.checks()[3].status(), Status::Ok);

        fake.variables.remove("DISPLAY");
        fake.variables
            .insert("WAYLAND_DISPLAY".into(), "wayland-0".into());
        let incomplete_wayland = examine_with(&fake, None);
        assert_eq!(incomplete_wayland.checks()[3].status(), Status::Warn);
        fake.variables
            .insert("XDG_RUNTIME_DIR".into(), "/run/user/1000".into());
        let wayland = examine_with(&fake, None);
        assert_eq!(wayland.checks()[3].status(), Status::Ok);
    }

    #[test]
    fn output_is_stable_and_carries_help_and_codes() {
        let mut fake = Fake::healthy();
        fake.variables.insert("CI".into(), "1".into());

        let output = examine_with(&fake, Some(PathBuf::from("/project"))).to_string();

        assert!(output.starts_with(
            "[OK] versions: CLI 0.1.0, runtime 1.25.1, adapter 0.1.0, iced 0.15.0-dev @ be88873d; linked together at build time\n[OK] renderer: compiled: wgpu, tiny-skia\n[OK] window system: compiled: AppKit\n[WARN] display session:"
        ));
        assert!(output.contains("code: RAWR-ENVIRONMENT-DOCTOR"));
        assert!(output.contains("help: run Rawr from an interactive desktop session"));
        assert!(output.ends_with(
            "[OK] extensions: no extension-pack search path configured; extension packs are optional\n"
        ));
    }

    fn missing_project() -> Discovery {
        Discovery::Failed(Failure {
            report: diagnostic::Report::error(
                diagnostic::code::ENVIRONMENT_PROJECT_NOT_FOUND,
                "no Rawr project found",
            )
            .finish(),
            component: Component::General,
            missing: true,
            root: None,
            source: None,
            manifest: None,
        })
    }

    fn component_failure(component: Component, message: &str) -> Discovery {
        Discovery::Failed(Failure {
            report: diagnostic::Report::error(
                diagnostic::code::ENVIRONMENT_UNREADABLE,
                message.to_owned(),
            )
            .help("fix permissions or UTF-8 encoding")
            .finish(),
            component,
            missing: false,
            root: None,
            source: None,
            manifest: None,
        })
    }
}

//! Integrated compilation and native hosting for local Rawr previews.

pub(crate) mod catalog;
pub(crate) mod chrome;
pub(crate) mod host;
mod theme;

use std::fmt;
use std::path::{Path, PathBuf};

use rawr::{diagnostic, preview, program, project, timing, Engine, ParseError, Shared};

/// A compiler with the canonical static iced facade installed.
pub struct Compiler {
    engine: Shared<Engine>,
}

/// One preview Catalog checked from an exact source graph.
pub struct Checked {
    engine: Shared<Engine>,
    catalog: preview::Catalog,
    source: diagnostic::Source,
    root: Option<PathBuf>,
    timing: Option<timing::Session>,
}

/// A bound and transactionally booted native preview host.
pub struct Prepared {
    control: crate::control::Host,
    source: diagnostic::Source,
}

/// Failure while checking, binding, booting, or running previews.
#[derive(Debug)]
pub enum Error {
    /// Exact diagnostic source evidence could not be retained.
    Diagnostic(diagnostic::Error),
    /// The static iced facade could not be installed.
    StaticFacade(rawr::host::Error),
    /// One in-memory source did not compile to a preview Catalog.
    Compile(ParseError),
    /// A package source graph did not compile to a preview Catalog.
    Package(rawr::package::Error),
    /// The Catalog did not belong to this compiler Engine.
    Launch(program::Error),
    /// The native iced facade could not be built or installed.
    RuntimeFacade(rawr::host::Error),
    /// Exact native bindings did not satisfy the Catalog.
    RuntimeBinding(rawr::host::runtime::Error),
    /// The initial selected session could not boot transactionally.
    Boot(diagnostic::Report),
    /// The native iced event loop failed.
    Native(iced::Error),
}

impl Compiler {
    /// Build a preview compiler with the exact same static facade as `rawr run`.
    pub fn new() -> Result<Self, Error> {
        let mut engine = Engine::new();
        crate::install(&mut engine).map_err(Error::StaticFacade)?;
        Ok(Self {
            engine: Shared::new(engine),
        })
    }

    /// Build a preview compiler, then let the caller tighten its Engine.
    ///
    /// Hosts that evaluate untrusted snippets use this to set operation,
    /// call-depth, and data-size limits and a progress (deadline) callback.
    pub fn configured(configure: impl FnOnce(&mut Engine)) -> Result<Self, Error> {
        let mut engine = Engine::new();
        crate::install(&mut engine).map_err(Error::StaticFacade)?;
        configure(&mut engine);
        Ok(Self {
            engine: Shared::new(engine),
        })
    }

    /// Check one discovered package target as a preview Catalog.
    pub fn check(&self, target: &project::Target) -> Result<Checked, Error> {
        let source = target_source(target)?;
        let catalog = self
            .engine
            .compile_package_preview_catalog(target)
            .map_err(Error::Package)?;
        Ok(Checked {
            engine: self.engine.clone(),
            catalog,
            source,
            root: target_root(target),
            timing: None,
        })
    }

    /// Check one discovered package target while retaining compiler timing.
    pub fn check_observed(&self, target: &project::Target) -> Result<Checked, Error> {
        let source = target_source(target)?;
        let timing = timing::Session::start();
        let catalog = self
            .engine
            .compile_package_preview_catalog_observed(target, &timing)
            .map_err(Error::Package)?;
        Ok(Checked {
            engine: self.engine.clone(),
            catalog,
            source,
            root: target_root(target),
            timing: Some(timing),
        })
    }

    /// Check one named in-memory source as a preview Catalog.
    pub fn check_named_source(
        &self,
        identity: impl Into<rawr::ImmutableString>,
        source: impl AsRef<str>,
    ) -> Result<Checked, Error> {
        let source =
            diagnostic::Source::new(identity, source.as_ref()).map_err(Error::Diagnostic)?;
        let catalog = self
            .engine
            .compile_preview_catalog_source(source.clone())
            .map_err(Error::Compile)?;
        Ok(Checked {
            engine: self.engine.clone(),
            catalog,
            source,
            root: None,
            timing: None,
        })
    }
}

impl Checked {
    /// Bind the whole Catalog once and boot its initial selection transactionally.
    pub fn prepare(self) -> Result<Prepared, Error> {
        let Bound {
            catalog,
            source,
            root,
            timing,
        } = self.bind()?;
        let state =
            host::Host::new(catalog.clone(), source.clone(), timing).map_err(Error::Boot)?;
        Ok(Prepared {
            control: crate::control::Host::new(state, catalog, root),
            source,
        })
    }

    /// Bind this exact Catalog into a deterministic snapshot suite.
    pub fn snapshots(self) -> Result<crate::snapshot::Suite, crate::snapshot::Error> {
        let bound = self.bind().map_err(crate::snapshot::Error::Preview)?;
        let root = bound.root.ok_or(crate::snapshot::Error::MissingRoot)?;
        Ok(crate::snapshot::Suite::new(bound.catalog, root))
    }

    /// Bind this Catalog and headlessly render its first preview to a PNG.
    ///
    /// Uses the same pinned tiny-skia renderer, font, and scale as
    /// `rawr snapshot`, with the preview's first declared theme (or the
    /// native theme when it declares none). Nothing touches the filesystem.
    pub fn render_png(self) -> Result<crate::snapshot::Png, crate::snapshot::Error> {
        let bound = self.bind().map_err(crate::snapshot::Error::Preview)?;
        crate::snapshot::render_first(bound.catalog, None)
    }

    /// Like [`Self::render_png`], sized and scaled by `canvas` (fit to the
    /// content, a margin, HiDPI), for one-off images such as `?play`.
    pub fn render_png_with(
        self,
        canvas: crate::snapshot::Canvas,
    ) -> Result<crate::snapshot::Png, crate::snapshot::Error> {
        let bound = self.bind().map_err(crate::snapshot::Error::Preview)?;
        crate::snapshot::render_first(bound.catalog, Some(canvas))
    }

    fn bind(self) -> Result<Bound, Error> {
        let launch = self
            .engine
            .launch_previews(self.catalog)
            .map_err(Error::Launch)?;
        let mut registry = rawr::host::runtime::Registry::new();
        for facade in crate::runtime_facades().map_err(Error::RuntimeFacade)? {
            registry.install(facade).map_err(Error::RuntimeBinding)?;
        }
        let catalog = std::sync::Arc::new(launch.bind(registry).map_err(Error::RuntimeBinding)?);
        Ok(Bound {
            catalog,
            source: self.source,
            root: self.root,
            timing: self.timing,
        })
    }
}

struct Bound {
    catalog: std::sync::Arc<preview::BoundCatalog>,
    source: diagnostic::Source,
    root: Option<PathBuf>,
    timing: Option<timing::Session>,
}

impl Prepared {
    /// Return the primary source snapshot used for host-level diagnostics.
    #[must_use]
    pub const fn source(&self) -> &diagnostic::Source {
        &self.source
    }

    /// Borrow the sole in-process owner of this prepared preview host.
    pub const fn control(&mut self) -> &mut crate::control::Host {
        &mut self.control
    }

    /// Consume this prepared host into its in-process control owner.
    #[must_use]
    pub fn into_control(self) -> crate::control::Host {
        self.control
    }

    /// Open the low-chrome native preview selector and run until it closes.
    pub fn run(self) -> Result<(), Error> {
        self.control.run().map_err(Error::Native)
    }
}

impl Error {
    /// Transform this preview-host failure into the canonical shared report.
    #[must_use]
    pub fn report(&self, source: Option<diagnostic::Source>) -> diagnostic::Report {
        let fallback = source.clone();
        let result = match self {
            Self::Compile(error) => source
                .ok_or(diagnostic::Error::EmptySourceIdentity)
                .and_then(|source| diagnostic::Report::from_parse(source, error)),
            Self::Package(error) => {
                return error.report().unwrap_or_else(|report_error| {
                    report_with_source(
                        diagnostic::code::ENVIRONMENT_DIAGNOSTIC,
                        format!("cannot retain package diagnostic evidence: {report_error}"),
                        source,
                    )
                });
            }
            Self::Launch(error) => diagnostic::Report::from_program(source, error),
            Self::Boot(report) => return report.clone(),
            Self::StaticFacade(error) | Self::RuntimeFacade(error) => Ok(report_with_source(
                diagnostic::code::FRAMEWORK_FACADE,
                error.to_string(),
                source,
            )),
            Self::RuntimeBinding(error) => Ok(report_with_source(
                diagnostic::code::FRAMEWORK_RUNTIME_BINDING,
                error.to_string(),
                source,
            )),
            Self::Native(error) => Ok(report_with_source(
                diagnostic::code::FRAMEWORK_NATIVE,
                error.to_string(),
                source,
            )),
            Self::Diagnostic(error) => Ok(report_with_source(
                diagnostic::code::ENVIRONMENT_DIAGNOSTIC,
                error.to_string(),
                source,
            )),
        };
        result.unwrap_or_else(|error| {
            let builder = diagnostic::Report::error(
                diagnostic::code::ENVIRONMENT_DIAGNOSTIC,
                format!("cannot retain exact diagnostic evidence: {error}"),
            );
            match fallback {
                Some(source) => builder.source(source).finish(),
                None => builder.finish(),
            }
        })
    }
}

fn target_source(target: &project::Target) -> Result<diagnostic::Source, Error> {
    diagnostic::Source::new(
        target.source().path().to_string_lossy().into_owned(),
        target.source().text(),
    )
    .map_err(Error::Diagnostic)
}

fn target_root(target: &project::Target) -> Option<PathBuf> {
    target
        .project()
        .map(|project| project.root().as_path().to_path_buf())
        .or_else(|| target.source().path().parent().map(Path::to_path_buf))
}

fn report_with_source(
    code: diagnostic::code::Code,
    message: impl Into<rawr::ImmutableString>,
    source: Option<diagnostic::Source>,
) -> diagnostic::Report {
    let builder = diagnostic::Report::error(code, message);
    match source {
        Some(source) => builder.source(source).finish(),
        None => builder.finish(),
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.report(None).fmt(formatter)
    }
}

impl std::error::Error for Error {}

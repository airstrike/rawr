//! Product transform from one `[rust]` package to a generated iced runner request.

use std::fmt;
use std::path::PathBuf;

use rawr_import::cargo::config::Environment;

#[cfg(all(test, feature = "import-baseline"))]
mod baseline;
pub(crate) mod owner;

/// One generated-owner request paired with its exact runner snapshot.
pub(crate) struct Prepared {
    request: rawr_import::bridge::GenerationRequestKey<rawr::package::import::Brand>,
    snapshot: rawr_import::supervisor::snapshot::Bytes,
    project: PathBuf,
    inputs: rawr::reload::source::Inputs,
    manifests: owner::resident::Manifests,
    limits: rawr_import::cargo::Limits,
}

/// Failure while transforming package evidence into one generated owner.
#[derive(Debug)]
pub(crate) enum Error {
    Rawr(rawr::package::Error),
    Brand(rawr::package::import::Error),
    Import(rawr_import::Error),
    ProfileDefinition(rawr_import::profile::Error),
    ProfileClassify(rawr_import::profile::Error),
    ProfileGenerate(rawr_import::profile::Error),
    Checked(rawr_import::checked::Error),
    CheckedPayload,
    Surface(rawr_import::surface::Error),
    Bridge(rawr_import::bridge::request::Error),
    Compiler(rawr_iced::profile::runner::Error),
    Snapshot(rawr_import::supervisor::snapshot::Error),
}

/// Discover one target and return it only when its application package owns a
/// validated `[rust]` import declaration.
pub(crate) fn declared(
    path: Option<&PathBuf>,
) -> Result<Option<rawr::project::Target>, rawr::diagnostic::Report> {
    let target = match path {
        Some(path) => rawr::project::Target::discover(path),
        None => rawr::project::Target::current(),
    }
    .map_err(|error| rawr::diagnostic::Report::from_project(&error))?;
    let fallback = rawr::diagnostic::Source::new(
        target.source().path().to_string_lossy().into_owned(),
        target.source().text(),
    )
    .map_err(|error| {
        crate::watch::report(rawr::diagnostic::code::ENVIRONMENT_DIAGNOSTIC, error, None)
    })?;
    let graph = rawr::package::Graph::load(&rawr::Engine::new(), &target)
        .map_err(|error| crate::watch::package_report(&error, fallback))?;
    Ok(graph.declares_rust_import().then_some(target))
}

/// Rawr compiles a project's Rust in the project's own build directory with
/// the pinned nightly, for the host named as a target. Those are the units
/// `cargo build` compiles only when the project builds the same way, which
/// is what `rawr new` sets up; otherwise every crate would be compiled twice.
/// `Err` names what the project needs, as the files to add.
pub(crate) fn same_build(root: &std::path::Path) -> Result<(), String> {
    let environment = Environment::capture().map_err(|error| error.to_string())?;
    // rustup decides the toolchain from the directory: `RUSTUP_TOOLCHAIN`,
    // an override, a `rust-toolchain.toml` above it, or the default.
    let active = std::process::Command::new("rustup")
        .args(["show", "active-toolchain"])
        .current_dir(root)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .split_whitespace()
                .next()
                .unwrap_or_default()
                .to_owned()
        });
    let target = rawr_import::location::build_target(root, &environment);
    let overrides = rawr_import::location::rawr_profile_configs(root, &environment);
    same_build_as(root, active.as_deref(), target.as_deref(), &overrides)
}

/// [`same_build`] for a project whose toolchain is `active` (`None` when
/// rustup cannot tell), whose `cargo build` target is `target`, and whose
/// Cargo config files `overrides` set a profile for the `rawr` crate.
fn same_build_as(
    root: &std::path::Path,
    active: Option<&str>,
    target: Option<&str>,
    overrides: &[PathBuf],
) -> Result<(), String> {
    let channel = rawr_import::cargo::toolchain::CHANNEL;
    let host = rawr_import::cargo::toolchain::HOST.trim();
    let mut needed = Vec::new();
    if let Some(active) = active
        && active != channel
        && !active.starts_with(&format!("{channel}-"))
    {
        needed.push(format!(
            "It builds with `{active}`, so add `rust-toolchain.toml`:\n\n{}",
            crate::new::toolchain()
        ));
    }
    if !matches!(target, Some(target) if target == host || target == "host-tuple") {
        needed.push(format!(
            "It builds for {}, so add `.cargo/config.toml`:\n\n{}",
            target.map_or_else(|| "no named target".to_owned(), |target| format!("`{target}`")),
            crate::new::cargo_config()
        ));
    }
    for config in overrides {
        needed.push(format!(
            "`{}` sets a profile for the `rawr` crate, and Cargo cannot combine it with \
             the one Rawr's runner sets. A project inside a Rawr checkout inherits the \
             checkout's config; move the project out of it, or remove that profile.",
            config.display()
        ));
    }
    if needed.is_empty() {
        return Ok(());
    }
    Err(format!(
        "`{}` must build as Rawr builds it, with `{channel}` for `{host}`, or each of its \
         crates is compiled twice: once by `cargo build` and once by Rawr. {}",
        root.display(),
        needed.join("\n")
    ))
}

/// The rawr-iced widget features a project's runner builds with.
///
/// The aim is the ones the project's iced has, as its Cargo.toml names them
/// (`image-without-codecs` counts as `image`), so the runner adds no iced
/// feature. Until the profile's compatibility pin (`COMPATIBILITY_DIGEST`
/// and `COMPATIBILITY_PROJECTIONS` in rawr-iced's `profile/runner.rs`)
/// holds for a facade without some widgets, every project gets them all.
fn widgets(graph: &rawr::package::Graph) -> Vec<rawr_iced::features::Feature> {
    const FOLLOW_PROJECT: bool = false;
    if !FOLLOW_PROJECT {
        return rawr_iced::features::Feature::ALL.to_vec();
    }
    let named = graph.dependency_features(rawr_iced::profile::Definition::FRAMEWORK);
    rawr_iced::features::Feature::ALL
        .into_iter()
        .filter(|feature| {
            named
                .iter()
                .any(|name| rawr_iced::features::Feature::of_iced(name) == Some(*feature))
        })
        .collect()
}

impl Prepared {
    /// The limits profile this preparation captured; later routes reuse it.
    pub(crate) const fn limits(&self) -> rawr_import::cargo::Limits {
        self.limits
    }

    /// Consume the proof bundle into the build request and exact source transport.
    pub(crate) fn into_parts(
        self,
    ) -> (
        rawr_import::bridge::GenerationRequestKey<rawr::package::import::Brand>,
        rawr_import::supervisor::snapshot::Bytes,
        PathBuf,
        rawr::reload::source::Inputs,
    ) {
        (self.request, self.snapshot, self.project, self.inputs)
    }
}

/// Resolve, classify, compile, and seal the exact generated iced owner.
#[cfg(test)]
pub(crate) fn prepare(
    target: &rawr::project::Target,
    revision: rawr_import::supervisor::Revision,
    #[cfg(feature = "import-baseline")] profile: Option<rawr_iced::profile::Fixture>,
) -> Result<Prepared, Error> {
    prepare_loaded(
        target,
        None,
        revision,
        #[cfg(feature = "import-baseline")]
        profile,
    )
}

/// [`prepare`] with the target's load when the caller already has it.
pub(crate) fn prepare_loaded(
    target: &rawr::project::Target,
    graph: Option<rawr::package::Graph>,
    revision: rawr_import::supervisor::Revision,
    #[cfg(feature = "import-baseline")] profile: Option<rawr_iced::profile::Fixture>,
) -> Result<Prepared, Error> {
    prepare_in(
        target,
        graph,
        revision,
        #[cfg(feature = "import-baseline")]
        profile,
        &rawr_import::supervisor::snapshot::attempt::Scope::begin(),
    )
}

fn prepare_in(
    target: &rawr::project::Target,
    graph: Option<rawr::package::Graph>,
    revision: rawr_import::supervisor::Revision,
    #[cfg(feature = "import-baseline")] profile: Option<rawr_iced::profile::Fixture>,
    scope: &rawr_import::supervisor::snapshot::attempt::Scope,
) -> Result<Prepared, Error> {
    prepare_transform(
        target,
        graph,
        revision,
        #[cfg(feature = "import-baseline")]
        profile,
        |_, _| Ok(()),
        scope,
    )
}

fn prepare_transform(
    target: &rawr::project::Target,
    graph: Option<rawr::package::Graph>,
    revision: rawr_import::supervisor::Revision,
    #[cfg(feature = "import-baseline")] profile: Option<rawr_iced::profile::Fixture>,
    verify_surface: impl FnOnce(&[u8], &rawr_import::cargo::Limits) -> Result<(), Error>,
    scope: &rawr_import::supervisor::snapshot::attempt::Scope,
) -> Result<Prepared, Error> {
    #[cfg(feature = "import-baseline")]
    let profile_selection = match profile {
        None => rawr_import::metrics::profile::Selection::Inactive,
        Some(rawr_iced::profile::Fixture::Baseline) => {
            rawr_import::metrics::profile::Selection::Baseline
        }
        Some(rawr_iced::profile::Fixture::Variant) => {
            rawr_import::metrics::profile::Selection::Variant
        }
    };
    let graph = match graph {
        Some(graph) => graph,
        None => rawr::package::Graph::load(&rawr::Engine::new(), target).map_err(Error::Rawr)?,
    };
    // The project's `[rust.limits]` accounting targets (standard when
    // unconfigured) are one captured profile for every route of this import.
    let limits = graph.rust_limits();
    let manifests = owner::resident::Manifests::from_graph(&graph);
    let source = rawr::reload::source::Graph::from_package(&graph);
    let snapshot = rawr::reload::source::Snapshot::from_graph(&source);
    let expected = snapshot.imported(&limits).map_err(Error::Surface)?;
    let transport = snapshot
        .imported_transport(expected, &limits)
        .map_err(Error::Snapshot)?;
    let project = graph.entry_package().root().to_path_buf();

    #[cfg(feature = "import-baseline")]
    let definition = match profile {
        Some(profile) => rawr_iced::profile::Definition::fixture(profile, &limits),
        None => rawr_iced::profile::Definition::new(&limits),
    }
    .map_err(Error::ProfileDefinition)?;
    #[cfg(not(feature = "import-baseline"))]
    let definition =
        rawr_iced::profile::Definition::new(&limits).map_err(Error::ProfileDefinition)?;
    let adapter = definition.trusted_adapter();
    let policy = rawr_import::classify::Policy::standard();
    let mut verify_surface = Some(verify_surface);
    let mut snapshot = Some(snapshot);
    // A Rust path Rawr names below a wholesale crate re-export that
    // extraction only named (`iced::futures::…`) is demanded, and the import
    // runs again with it, once per new path. The demand is remembered beside
    // the project cache, so each preparation starts from it.
    let mut crate_demand = rawr_import::crate_demand::load(&project);
    let mut remembered = !crate_demand.is_empty();
    // A dependency the Rawr source names is extracted from what
    // it names (a syntactic pass over `use` and qualified paths, plus the
    // paths the iced profile selects), so the first launch needs one probe.
    // A path the pass misses below such a dependency is demanded as above.
    let framework = rawr_iced::profile::Definition::FRAMEWORK;
    // The runner's rawr-iced has the widgets the project's iced has, and
    // this process declares the same facade to check against.
    let widgets = widgets(&graph);
    rawr_iced::features::select(widgets.iter().copied());
    let seed = graph.rust_demand_seed(rawr_iced::profile::Definition::DEMAND, Some(framework));
    #[cfg(feature = "import-baseline")]
    let (mut observed_toolchain, mut observed_resolution);
    #[cfg(feature = "import-baseline")]
    let (mut observed_api, mut observed_generic): ([u8; 32], [u8; 32]);
    let (demand, profile_contract, facade, cached, native) = loop {
        let mut request_demand = crate_demand.clone();
        request_demand.extend(seed.iter().cloned());
        request_demand.sort_unstable();
        request_demand.dedup();
        let environment = Environment::capture().map_err(Error::Import)?;
        let dependency = rawr_iced::profile::Definition::cargo_dependency(&widgets, &limits)
            .map_err(Error::Import)?;
        let framework_dependency =
            rawr_iced::profile::Definition::framework_dependency(&limits).map_err(Error::Import)?;
        let request = match graph.rust_import_with_profile(
            environment,
            dependency,
            Some((framework, framework_dependency)),
            definition.resolution_universe(),
            &request_demand,
            &limits,
        ) {
            Ok(Some(request)) => request,
            Ok(None) => {
                return Err(Error::Import(rawr_import::Error::Request {
                    field: "Rust imports",
                    reason: "the crate has no Rust dependency or native module".into(),
                }));
            }
            // A remembered path the manifest no longer admits (a removed
            // dependency alias) is forgotten, never a failed launch.
            Err(_) if remembered => {
                remembered = false;
                crate_demand.clear();
                rawr_import::crate_demand::store(&project, &crate_demand);
                continue;
            }
            Err(error) => return Err(Error::Import(error)),
        };
        remembered = false;
        let prepared = request
            .prepare_cached_in(&limits, scope)
            .map_err(Error::Import)?;
        #[cfg(feature = "import-baseline")]
        {
            observed_toolchain = prepared.observed_toolchain();
            observed_resolution = prepared.observed_identity();
        }
        let api = prepared.retain_api(&limits).map_err(Error::Import)?;
        let snapshot = snapshot
            .take()
            .unwrap_or_else(|| rawr::reload::source::Snapshot::from_graph(&source));
        // Unchanged inputs reuse the sealed resolved demand: no API graph rebuild,
        // no classification replay. Any change misses and runs the full path.
        let warm = rawr_import::composition::resolve_api_semantic(
            api,
            &adapter,
            &policy,
            expected,
            &limits,
            |facade| rawr::package::import::Brand::replay(facade).map_err(Error::Brand),
        );
        let warm = match warm {
            Ok(warm) => warm,
            Err(rawr_import::checked::Evaluation::Transform(error)) => return Err(error),
            Err(rawr_import::checked::Evaluation::Cache(error)) => {
                return Err(Error::Checked(error))
            }
        };
        break match warm {
            Ok(warm) => {
                #[cfg(feature = "import-baseline")]
                {
                    observed_api = *warm.api.as_bytes();
                    observed_generic = *warm.demand.contract_fingerprint().as_bytes();
                }
                (
                    warm.demand,
                    warm.profile,
                    warm.facade,
                    warm.checked,
                    warm.native,
                )
            }
            Err(api) => {
                let completed = rawr_import::composition::evaluate_api_semantic(
                    api,
                    &adapter,
                    &policy,
                    expected,
                    &limits,
                    |contract, profile_contract, facade| {
                        if let Some(verify) = verify_surface.take() {
                            verify(facade.bytes(), &limits)?;
                        }
                        let mut engine = rawr::Engine::new();
                        let provisional = *profile_contract.rules_digest();
                        #[cfg(feature = "import-baseline")]
                        let compiler =
                            rawr_iced::profile::runner::install_compiler_contract_fixture(
                                &mut engine,
                                definition.selected_fixture(),
                                definition.selected_fixture().version(),
                                facade.bytes(),
                                provisional,
                                &profile_contract,
                            )
                            .map_err(Error::Compiler)?;
                        #[cfg(not(feature = "import-baseline"))]
                        let compiler = rawr_iced::profile::runner::install_compiler_contract(
                            &mut engine,
                            facade.bytes(),
                            provisional,
                            profile_contract,
                        )
                        .map_err(Error::Compiler)?;
                        let event_projection = profile_contract
                            .event_projection_rules()
                            .map_err(Error::ProfileClassify)?;
                        let application = engine
                            .compile_imported_application_with_installed_profile(
                                contract,
                                snapshot,
                                compiler.application_cycle(),
                                event_projection,
                            )
                            .map_err(Error::Rawr)?;
                        // The full demand is known: forget remembered paths
                        // the Rawr source no longer names.
                        let named = crate_demand
                            .iter()
                            .filter(|path| application.names(path))
                            .cloned()
                            .collect::<Vec<_>>();
                        if named.len() != crate_demand.len() {
                            rawr_import::crate_demand::store(&project, &named);
                        }
                        let (demand, application) = application.into_parts();
                        let checked = rawr_iced::profile::Checked::from_checked_contract(
                            &application,
                            compiler.application_cycle(),
                        );
                        Ok((demand, checked))
                    },
                );
                let completed = match completed {
                    Ok(completed) => completed,
                    Err(rawr_import::checked::Evaluation::Transform(Error::Rawr(error)))
                        if new_demand(&error, &request_demand).is_some() =>
                    {
                        crate_demand.extend(new_demand(&error, &request_demand).map(Box::from));
                        crate_demand.sort_unstable();
                        rawr_import::crate_demand::store(&project, &crate_demand);
                        continue;
                    }
                    Err(rawr_import::checked::Evaluation::Transform(error)) => return Err(error),
                    Err(rawr_import::checked::Evaluation::Cache(error)) => {
                        return Err(Error::Checked(error));
                    }
                };
                let rawr_import::composition::Completed {
                    contract,
                    profile: profile_contract,
                    facade,
                    checked: cached,
                } = completed;
                #[cfg(feature = "import-baseline")]
                {
                    observed_api = *contract.raw_fingerprint().as_bytes();
                    observed_generic = *contract.fingerprint().as_bytes();
                }
                let brand = rawr::package::import::Brand::replay(&facade).map_err(Error::Brand)?;
                let demand = cached
                    .replay_demand(brand, &contract, expected, &limits)
                    .map_err(Error::Checked)?
                    // The shipped Brand compiles snapshots with this exact owned facade.
                    // Capture those inputs explicitly; ordinary Runner callbacks remain
                    // on the conservative request path.
                    .with_trusted_runner(true, &facade, &contract)
                    .map_err(Error::Surface)?;
                rawr_import::composition::record_resolved(&contract, &demand, &adapter, &policy)
                    .map_err(Error::Checked)?;
                (
                    demand,
                    profile_contract,
                    facade,
                    cached,
                    contract.into_native_proof(),
                )
            }
        };
    };
    #[cfg(feature = "import-baseline")]
    let observed_facade = rawr_import::metrics::pipeline::digest(b"facade", facade.bytes());
    if let Some(verify) = verify_surface.take() {
        verify(facade.bytes(), &limits)?;
    }
    #[cfg(feature = "import-baseline")]
    let observed_checked: [u8; 32] = cached
        .checked_payload()
        .try_into()
        .map_err(|_| Error::CheckedPayload)?;
    rawr_iced::profile::Checked::from_retained(&cached).ok_or(Error::CheckedPayload)?;
    let profile = rawr_import::composition::profile::prepare_retained(
        &adapter,
        &profile_contract,
        &cached,
        &limits,
    )
    .map_err(Error::ProfileGenerate)?;
    #[cfg(feature = "import-baseline")]
    let observed_profile = *profile.demand_digest();
    #[cfg(feature = "import-baseline")]
    let observed_profile_proof = *profile.fingerprint();
    #[cfg(feature = "import-baseline")]
    rawr_import::metrics::record_profile(&rawr_import::metrics::profile::Evidence::observe(
        profile_selection,
        *profile_contract.identity().digest(),
        profile_contract.version().value(),
        *profile.rules_digest(),
    ));
    #[cfg(feature = "import-baseline")]
    let observed_native_inputs = *native.observed_fingerprint().map_err(Error::Import)?;
    #[cfg(feature = "import-baseline")]
    let observed_native_product = rawr_import::metrics::pipeline::digest_pair(
        b"native-product",
        &observed_native_inputs,
        &observed_profile_proof,
    );
    let request = demand
        .request_retained(native, profile, revision)
        .map_err(Error::Bridge)?;
    #[cfg(feature = "import-baseline")]
    rawr_import::metrics::record_pipeline(&rawr_import::metrics::pipeline::Evidence {
        toolchain: observed_toolchain,
        resolution: observed_resolution,
        api: observed_api,
        generic: observed_generic,
        profile: observed_profile,
        facade: observed_facade,
        checked: observed_checked,
        profile_proof: observed_profile_proof,
        native_inputs: observed_native_inputs,
        native_product: observed_native_product,
        overlay: *request.build_request_key().as_bytes(),
        resolution_target: None,
        runner_target: None,
    });
    let inputs = rawr::reload::source::Inputs::from_graph(&source).with_imported(request.watch());
    Ok(Prepared {
        request,
        snapshot: transport,
        project,
        inputs,
        manifests,
        limits,
    })
}

/// The Rust path a failed compilation missed that a new preparation may
/// resolve: one below a wholesale crate re-export, or one below a
/// dependency extracted from demand, when not yet demanded.
/// A dependency nothing demands is extracted whole and has no more to give.
fn new_demand<'error>(
    error: &'error rawr::package::Error,
    demand: &[Box<str>],
) -> Option<&'error str> {
    let root = |path: &str| path.split("::").next().map(str::to_owned);
    let path = error.rust_demand().or_else(|| {
        error
            .rust_miss()
            .filter(|path| demand.iter().any(|known| root(known) == root(path)))
    })?;
    (!demand.iter().any(|known| **known == *path)).then_some(path)
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Rawr(error) => error.fmt(formatter),
            Self::Brand(error) => error.fmt(formatter),
            Self::Import(error) => error.fmt(formatter),
            Self::ProfileDefinition(error) => write!(formatter, "iced profile definition: {error}"),
            Self::ProfileClassify(error) => {
                write!(formatter, "iced profile classification: {error}")
            }
            Self::ProfileGenerate(error) => write!(formatter, "iced profile generation: {error}"),
            Self::Checked(error) => write!(formatter, "checked application cache: {error}"),
            Self::CheckedPayload => {
                formatter.write_str("cached iced checked payload has an invalid shape")
            }
            Self::Surface(error) => error.fmt(formatter),
            Self::Bridge(error) => error.fmt(formatter),
            Self::Compiler(error) => error.fmt(formatter),
            Self::Snapshot(error) => error.fmt(formatter),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Rawr(error) => Some(error),
            Self::Brand(error) => Some(error),
            Self::Import(error) => Some(error),
            Self::ProfileDefinition(error)
            | Self::ProfileClassify(error)
            | Self::ProfileGenerate(error) => Some(error),
            Self::Checked(error) => Some(error),
            Self::Surface(error) => Some(error),
            Self::Bridge(error) => Some(error),
            Self::Compiler(error) => Some(error),
            Self::Snapshot(error) => Some(error),
            Self::CheckedPayload => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::{Path, PathBuf};

    use rawr_import::cargo::config::Environment;

    #[test]
    fn a_project_builds_as_rawr_builds_it_or_is_told_what_to_add() {
        let root = Path::new("/project");
        let channel = rawr_import::cargo::toolchain::CHANNEL;
        let host = rawr_import::cargo::toolchain::HOST.trim();
        let pinned = format!("{channel}-{host}");
        assert_eq!(super::same_build_as(root, Some(&pinned), Some(host), &[]), Ok(()));
        assert_eq!(super::same_build_as(root, None, Some(host), &[]), Ok(()));
        assert_eq!(super::same_build_as(root, None, Some("host-tuple"), &[]), Ok(()));
        let error = super::same_build_as(root, Some(&pinned), None, &[]).unwrap_err();
        assert!(error.contains(".cargo/config.toml"), "{error}");
        assert!(!error.contains("rust-toolchain.toml"), "{error}");
        let error = super::same_build_as(root, Some("stable-aarch64-apple-darwin"), Some(host), &[])
            .unwrap_err();
        assert!(error.contains("rust-toolchain.toml"), "{error}");
        assert!(error.contains(channel), "{error}");
        let config = PathBuf::from("/checkout/.cargo/config.toml");
        let error = super::same_build_as(root, Some(&pinned), Some(host), &[config]).unwrap_err();
        assert!(error.contains("/checkout/.cargo/config.toml"), "{error}");
    }

    #[test]
    fn real_iced_import_seals_one_generated_profile_owner() {
        rawr_import::store::isolate_for_tests();
        let project = real_iced_fixture();
        let root = project.path().to_path_buf();
        let target = rawr::project::Target::discover(&root).expect("fixture target");
        let mut owner = rawr_import::supervisor::Owner::new();
        let revision = owner.revision().expect("fixture revision");
        let prepared = super::prepare(
            &target,
            revision,
            #[cfg(feature = "import-baseline")]
            None,
        )
        .unwrap_or_else(|error| {
            panic!(
                "generated owner prepares: {error:?}; profile-selection-stage={:?}; canonical-selector={:?}",
                rawr_iced::profile::selection_diagnostic(),
                rawr_import::extract::canonical_selector_diagnostic(),
            )
        });
        let (request, _snapshot, _, _) = prepared.into_parts();
        let manifest = request
            .generate(&rawr_import::cargo::Limits::standard())
            .expect("generated owner materializes");
        let source = manifest
            .files()
            .find(|file| file.path() == "src/main.rs")
            .expect("generated source");
        let source = std::str::from_utf8(source.bytes()).expect("generated source UTF-8");
        assert!(source.contains("bind_profile_projections"));
        assert!(source.contains("insert_projectable"));
        assert!(source.contains("__rawr_profile_host::profile::runner::bind_generation"));
        assert!(source.contains("Err(reason) => reject(&mut stream, reason)?"));
        assert!(source.contains("host.snapshot().map_err(|error| error.to_string())?"));
        assert!(source.contains(".map_err(|error| error.to_string())"));
        assert!(source.contains(".bind_type_attestation(\"iced::Element\""));
        assert!(source.contains("native_attestation(value, expected)"));
        assert!(source.contains("__rawr_profile::attest_support_endpoint(value, expected)"));
        assert!(!source
            .contains(".bind_projection_with_context(\"iced::widget::Text\", \"iced::Element\"",));
        fs::remove_dir_all(root).ok();
    }

    #[test]
    #[ignore = "production cutover gate builds and admits the real imported iced owner"]
    fn production_check_and_snapshot_use_the_real_generated_iced_owner() {
        let project = real_iced_fixture();
        let root = project.path().to_path_buf();
        let checked = crate::check::execute(Some(root.clone())).unwrap_or_else(|error| {
            panic!(
                "production imported check rejects: {error}; profile-selection-stage={:?}; canonical-selector={:?}",
                rawr_iced::profile::selection_diagnostic(),
                rawr_import::extract::canonical_selector_diagnostic(),
            )
        });
        assert_eq!(checked, root.join("src/main.rs"));

        let updated = crate::snapshot::execute(crate::command::Mode::Update, Some(root.clone()))
            .unwrap_or_else(|error| panic!("production imported snapshot update rejects: {error}"));
        assert!(updated.is_successful());
        assert!(updated.updated());
        assert_eq!(updated.artifacts(), 1);
        let checked = crate::snapshot::execute(crate::command::Mode::Check, Some(root.clone()))
            .unwrap_or_else(|error| panic!("production imported snapshot check rejects: {error}"));
        assert!(checked.is_successful());
        assert_eq!(checked.artifacts(), 1);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    #[ignore = "one-run real iced canonicalization evidence"]
    fn measures_the_exact_real_iced_import_without_publication() {
        let project = real_iced_fixture();
        let root = project.path().to_path_buf();
        let limits = rawr_import::cargo::Limits::standard();
        let target = rawr::project::Target::discover(&root).expect("fixture target");
        let graph = rawr::package::Graph::load(&rawr::Engine::new(), &target)
            .expect("fixture package graph");
        let environment = Environment::capture().expect("fixture Cargo environment");
        let dependency = rawr_iced::profile::Definition::cargo_dependency(&super::widgets(&graph), &limits)
            .expect("fixture profile dependency");
        let universe = rawr_iced::profile::Definition::new(&limits)
            .expect("fixture profile definition")
            .resolution_universe();
        let request = graph
            .rust_import_with_profile(environment, dependency, None, universe, &[], &limits)
            .expect("fixture import request")
            .expect("fixture declares rust dependencies");
        let native = rawr_import::process::Native::pinned().expect("pinned native toolchain");
        let evidence = rawr_import::extract::measure_canonicalization(request, &native, &limits)
            .expect("real iced canonicalization completes inside the evidence envelope");
        eprintln!(
            "CANONICAL_SAMPLE items={} bytes={}",
            evidence.items(),
            evidence.bytes(),
        );
        assert!(evidence.items() <= 25_000_000);
        fs::remove_dir_all(root).ok();
    }

    #[test]
    #[ignore = "one-run real iced classification evidence"]
    fn measures_the_exact_real_iced_classification_without_publication() {
        let project = real_iced_fixture();
        let root = project.path().to_path_buf();
        let limits = rawr_import::cargo::Limits::standard();
        let target = rawr::project::Target::discover(&root).expect("fixture target");
        let graph = rawr::package::Graph::load(&rawr::Engine::new(), &target)
            .expect("fixture package graph");
        let environment = Environment::capture().expect("fixture Cargo environment");
        let dependency = rawr_iced::profile::Definition::cargo_dependency(&super::widgets(&graph), &limits)
            .expect("fixture profile dependency");
        let universe = rawr_iced::profile::Definition::new(&limits)
            .expect("fixture profile definition")
            .resolution_universe();
        let request = graph
            .rust_import_with_profile(environment, dependency, None, universe, &[], &limits)
            .expect("fixture import request")
            .expect("fixture declares rust dependencies");
        let native = rawr_import::process::Native::pinned().expect("pinned native toolchain");
        rawr_import::extract::measure_classification(request, &native, &limits)
            .expect("real iced classification completes under production authority");
        fs::remove_dir_all(root).ok();
    }

    #[test]
    #[ignore = "RawrGallery full-surface invocation-template qualification without production CLI cutover"]
    fn proves_rawrgallery_full_surface_before_production() {
        let Some(root) = std::env::var_os("RAWR_GALLERY").map(PathBuf::from) else {
            return;
        };
        let target = rawr::project::Target::discover(&root).expect("RawrGallery target");
        let mut owner = rawr_import::supervisor::Owner::new();
        let revision = owner.revision().expect("RawrGallery evidence revision");
        let prepared = super::prepare_transform(
            &target,
            None,
            revision,
            #[cfg(feature = "import-baseline")]
            None,
            |facade, limits| {
                rawr_import::surface::facade::Plan::verify_exact_surface(
                    facade,
                    limits,
                    "gallery_ui",
                    &["Dial", "domain::Screen", "domain::State"],
                    &[
                        "Dial::new",
                        "Dial::range",
                        "domain::DEFAULT_DIAL",
                        "domain::State::new",
                        "domain::State::select",
                        "domain::State::toggle",
                        "domain::apply",
                        "domain::clamp",
                        "domain::normalize",
                    ],
                    &["domain::UnsupportedScalar"],
                )
                .map_err(super::Error::Surface)?;
                let plan = rawr_import::surface::facade::Plan::decode(facade, limits)
                    .map_err(super::Error::Surface)?;
                let iced = plan
                    .facades()
                    .find(|facade| facade.root() == "iced")
                    .expect("qualified surface retains the exact iced facade");
                let assert_cohort = |anchor: &str, expected: &[&str]| {
                    let identity = iced
                        .types()
                        .find(|ty| ty.path() == anchor)
                        .expect("cohort anchor survives the sealed facade")
                        .identity();
                    let actual = iced
                        .types()
                        .filter(|ty| ty.identity() == identity)
                        .map(rawr_import::surface::facade::TypeDefinition::path)
                        .collect::<Vec<_>>();
                    assert_eq!(actual, expected, "{anchor} canonical alias cohort drifted");
                };
                assert_cohort(
                    "Backend",
                    &[
                        "Backend",
                        "advanced::graphics::core::Backend",
                        "advanced::graphics::core::backend::Backend",
                        "advanced::graphics::futures::core::Backend",
                        "advanced::graphics::futures::core::backend::Backend",
                        "backend::Backend",
                    ],
                );
                assert_cohort(
                    "Alignment",
                    &[
                        "Alignment",
                        "advanced::graphics::core::Alignment",
                        "advanced::graphics::core::alignment::Alignment",
                        "advanced::graphics::futures::core::Alignment",
                        "advanced::graphics::futures::core::alignment::Alignment",
                        "alignment::Alignment",
                    ],
                );
                Ok(())
            },
            &rawr_import::supervisor::snapshot::attempt::Scope::begin(),
        )
        .expect("RawrGallery full intended facade installs and compiles");
        let (request, snapshot, _, _) = prepared.into_parts();
        let manifest = request
            .generate(&rawr_import::cargo::Limits::standard())
            .expect("RawrGallery sealed demand generates");
        let source = manifest
            .files()
            .find(|file| file.path() == "src/main.rs")
            .expect("RawrGallery generated source");
        let source = std::str::from_utf8(source.bytes()).expect("generated source UTF-8");
        for path in [
            "gallery_ui::Dial::<i64>::new",
            "gallery_ui::domain::apply",
            "gallery_ui::domain::clamp",
            "gallery_ui::domain::normalize",
        ] {
            assert!(source.contains(path), "generated source omits {path}");
        }
        assert!(
            source.contains(".range"),
            "generated source omits Dial::range"
        );
        assert!(
            !source.contains("gallery_ui::Dial::new::<i64>"),
            "generated source misplaced an implementation-owned argument on the item"
        );
        assert!(source.contains("__rawr_profile::payload"));
        assert!(source.contains("profile_event_mapper"));
        assert!(source.contains("insert_projectable"));
        assert!(source.contains("static STRUCTURAL_ROWS"));
        assert!(source.contains("StructuralKind::Field { copy: true }"));
        assert!(source.contains("StructuralKind::Variant"));
        assert!(
            !source.contains("EXPECTED_APPLICATION_SEMANTIC"),
            "current application evidence must travel with the launch snapshot, not compiled owner source",
        );
        assert!(source.contains("compile_package_snapshot_generation_checked"));
        assert!(
            source.contains("arguments.arena.shared::<gallery_ui::domain::State"),
            "Copy scalar fields use one non-consuming exact-owner read",
        );

        let mut lane = crate::watch::native::Lane::new(&root)
            .expect("RawrGallery non-production qualification lane");
        let key = lane
            .replace(manifest, snapshot)
            .expect("RawrGallery generated owner admits complete intended surface");
        assert_eq!(lane.current_key(), Some(key));
        lane.shutdown()
            .expect("RawrGallery non-production qualification owner retires");
    }

    pub(super) fn real_iced_fixture() -> rawr_import::store::TestProject {
        let project = rawr_import::store::TestProject::new("rawr-cli-generated-owner");
        write_real_iced_fixture(project.path()).expect("fixture inputs");
        project
    }

    fn write_real_iced_fixture(root: &Path) -> std::io::Result<()> {
        fs::create_dir_all(root.join("src"))?;
        fs::write(
            root.join("Cargo.toml"),
            r#"[package]
name = "generated-owner"
version = "0.1.0"
edition = "2024"

[dependencies]
iced = { git = "https://github.com/iced-rs/iced", rev = "be88873d38544455e26c7a104eb757da2cbbac6b", default-features = false, features = ["advanced"] }

[workspace]
resolver = "3"
"#,
        )?;
        fs::write(
            root.join("src/main.rs"),
            r#"// rawr
mod native;

use iced::Element;
use iced::widget::text;

#[derive(Default)] struct State;
#[derive(Debug, Clone, Copy)] enum Message { Tick }

impl State {
    fn update(&mut self, message: Message) {}
    #[preview(name = "Default", size = (64, 48))]
    fn view(&self) -> Element<Message> { text("generated") }
}

pub fn main() -> iced::Result { iced::run(State::update, State::view) }
"#,
        )?;
        // An unmarked module: the crate builds natively, iced with it.
        fs::write(
            root.join("src/native.rs"),
            "pub fn label() -> &'static str {\n    \"generated\"\n}\n",
        )?;
        Ok(())
    }
}

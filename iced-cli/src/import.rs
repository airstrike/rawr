//! rawr-iced's profile: the transform from a package that uses iced to a
//! runner request whose runner links rawr-iced and the project's iced. The
//! runner's owner is rawr-import's; see [`rawr_import_cli::owner`].

use std::fmt;
use std::path::{Path, PathBuf};

use rawr_import::cargo::config::Environment;

pub(crate) mod owner;

pub(crate) use rawr_import_cli::Prepared;

/// rawr-iced's profile: the runner links rawr-iced as its profile host and
/// the project's iced, and the profile reads its evidence from that iced.
pub(crate) struct Iced;

impl rawr_import_cli::Profile for Iced {
    fn prepare(
        &self,
        target: &rawr::project::Target,
        graph: Option<rawr::package::Graph>,
        revision: rawr_import::supervisor::Revision,
        scope: &rawr_import::supervisor::snapshot::attempt::Scope,
    ) -> Result<Prepared, rawr_import_cli::Error> {
        prepare_in(target, graph, revision, scope).map_err(Into::into)
    }

    /// The window listens on an agent socket under `target/rawr`.
    fn socket(&self, project: &Path) -> Option<PathBuf> {
        Some(rawr_iced::reload::agent::socket(project))
    }
}

impl From<Error> for rawr_import_cli::Error {
    fn from(error: Error) -> Self {
        match error {
            Error::Rawr(error) => Self::Rawr(error),
            error => Self::other(error),
        }
    }
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

/// The rawr-iced widget features a project's runner builds with.
///
/// The aim is the ones the project's iced has, as its Cargo.toml names them
/// (`image-without-codecs` counts as `image`), so the runner adds no iced
/// feature. Until a runner is shown to build with the project's list alone,
/// every project gets them all.
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

/// Resolve, classify, compile, and seal the exact generated iced owner.
#[cfg(test)]
pub(crate) fn prepare(
    target: &rawr::project::Target,
    revision: rawr_import::supervisor::Revision,
) -> Result<Prepared, Error> {
    prepare_in(
        target,
        None,
        revision,
        &rawr_import::supervisor::snapshot::attempt::Scope::begin(),
    )
}

fn prepare_in(
    target: &rawr::project::Target,
    graph: Option<rawr::package::Graph>,
    revision: rawr_import::supervisor::Revision,
    scope: &rawr_import::supervisor::snapshot::attempt::Scope,
) -> Result<Prepared, Error> {
    let graph = match graph {
        Some(graph) => graph,
        None => rawr::package::Graph::load(&rawr::Engine::new(), target).map_err(Error::Rawr)?,
    };
    // The project's `[rust.limits]` accounting targets (standard when
    // unconfigured) are one captured profile for every route of this import.
    let limits = graph.rust_limits();
    let source = rawr::reload::source::Graph::from_package(&graph);
    let snapshot = rawr::reload::source::Snapshot::from_graph(&source);
    let expected = snapshot.imported(&limits).map_err(Error::Surface)?;
    let transport = snapshot
        .imported_transport(expected, &limits)
        .map_err(Error::Snapshot)?;
    let project = graph.entry_package().root().to_path_buf();

    let definition =
        rawr_iced::profile::Definition::new(&limits).map_err(Error::ProfileDefinition)?;
    let adapter = definition.trusted_adapter();
    let policy = rawr_import::classify::Policy::standard();
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
    let (demand, profile_contract, cached, native) = loop {
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
            Ok(warm) => (warm.demand, warm.profile, warm.checked, warm.native),
            Err(api) => {
                let completed = rawr_import::composition::evaluate_api_semantic(
                    api,
                    &adapter,
                    &policy,
                    expected,
                    &limits,
                    |contract, profile_contract, facade| {
                        let mut engine = rawr::Engine::new();
                        let provisional = *profile_contract.rules_digest();
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
                        if rawr_import_cli::new_demand(&error, &request_demand).is_some() =>
                    {
                        crate_demand.extend(
                            rawr_import_cli::new_demand(&error, &request_demand).map(Box::from),
                        );
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
                    cached,
                    contract.into_native_proof(),
                )
            }
        };
    };
    rawr_iced::profile::Checked::from_retained(&cached).ok_or(Error::CheckedPayload)?;
    let profile = rawr_import::composition::profile::prepare_retained(
        &adapter,
        &profile_contract,
        &cached,
        &limits,
    )
    .map_err(Error::ProfileGenerate)?;
    let request = demand
        .request_retained(native, profile, revision)
        .map_err(Error::Bridge)?;
    let inputs = rawr::reload::source::Inputs::from_graph(&source).with_imported(request.watch());
    Ok(Prepared::new(
        request, transport, project, inputs, &graph, limits,
    ))
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
    use std::path::Path;

    use rawr_import::cargo::config::Environment;

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
        let dependency =
            rawr_iced::profile::Definition::cargo_dependency(&super::widgets(&graph), &limits)
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
        let dependency =
            rawr_iced::profile::Definition::cargo_dependency(&super::widgets(&graph), &limits)
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

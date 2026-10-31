//! The profile of a program no host serves: its runner links the `rawr`
//! crate and the program's own dependencies, and nothing else. It resolves
//! in the importer's plain universe, and the runner starts the program by
//! calling its `main`.

use rawr_import::cargo::config::Environment;
use rawr_import::profile::{self, contract, demand, trusted};

/// Profile operations that select nothing: no evidence, demand or source.
struct Operations;

impl trusted::Operations for Operations {
    fn classify<P: rawr_import::api::Phase>(
        &self,
        _graph: &rawr_import::api::Graph<P>,
        _output: &mut contract::Builder<'_, P>,
    ) -> Result<(), profile::Error> {
        Ok(())
    }

    fn demand(
        &self,
        _contract: trusted::Contract<'_>,
        _checked: trusted::Checked<'_>,
        _output: &mut demand::Builder<'_>,
    ) -> Result<(), profile::Error> {
        Ok(())
    }

    fn generate(
        &self,
        _demand: trusted::Demand<'_>,
        _output: &mut demand::Generator<'_>,
    ) -> Result<(), profile::Error> {
        Ok(())
    }
}

/// The checked program a plain profile binds: the fingerprint of the
/// contract the program was checked against.
struct Checked([u8; 32]);

impl profile::Checked for Checked {
    fn profile_fingerprint(&self) -> [u8; 32] {
        self.0
    }

    fn profile_payload(&self) -> &[u8] {
        &self.0
    }
}

const IDENTITY: &str = "rawr.plain";
const VERSION: u32 = 1;

fn adapter(
    limits: &rawr_import::cargo::Limits,
) -> Result<trusted::Adapter<Operations>, profile::Error> {
    Ok(trusted::Adapter::trusted(
        Operations,
        profile::Identity::new(IDENTITY, limits)?,
        profile::Version::new(VERSION)?,
        false,
    )
    .semantic_api())
}

/// The profile that adds nothing to the runner.
#[derive(Clone, Copy, Debug, Default)]
pub struct Plain;

impl crate::Profile for Plain {
    fn prepare(
        &self,
        target: &rawr::project::Target,
        graph: Option<rawr::package::Graph>,
        revision: rawr_import::supervisor::Revision,
        scope: &rawr_import::supervisor::snapshot::attempt::Scope,
    ) -> Result<crate::Prepared, crate::Error> {
        prepare(target, graph, revision, scope)
    }
}

/// Why an imported call has no application.
enum Proof {
    /// Rust rejects the call, finds it ambiguous, or the closed rules
    /// cannot decide it.
    Rejected(String),
    /// Acquiring or joining the facts failed.
    Failed(crate::Error),
}

/// Prove one imported call from the dependencies' intrinsic facts and keep
/// its application. The facts are acquired once per preparation.
fn prove(
    graph: &rawr::package::Graph,
    limits: &rawr_import::cargo::Limits,
    call: &rawr_import::surface::application::Call,
    inventory: &mut Option<rawr_import::applicability::inventory::Acquired>,
    applications: &mut Vec<rawr_import::surface::application::Application>,
) -> Result<(), Proof> {
    use crate::Error;
    let acquired = match inventory {
        Some(acquired) => acquired,
        None => {
            let request = |closure: &[rawr_import::cargo::config::Dependency]| {
                let environment = Environment::capture()
                    .map_err(Error::other)
                    .map_err(Proof::Failed)?;
                graph
                    .rust_inventory_plain(environment, closure, limits)
                    .map_err(Error::other)
                    .map_err(Proof::Failed)?
                    .ok_or_else(|| Proof::Failed(Error::other("the crate has no Rust dependency")))
            };
            let probe = request(&[])?;
            let native = probe
                .pinned_native(limits)
                .map_err(Error::other)
                .map_err(Proof::Failed)?;
            // Every crate the build reaches is declared, so each crate that
            // can implement a trait has its own complete partition.
            let closure = probe
                .closure_dependencies(&native, limits)
                .map_err(Error::other)
                .map_err(Proof::Failed)?;
            let request = request(&closure)?;
            let acquired =
                rawr_import::applicability::inventory::Acquired::capture(request, &native, limits)
                    .map_err(Error::other)
                    .map_err(Proof::Failed)?;
            inventory.insert(acquired)
        }
    };
    let catalog = rawr_import::applicability::Catalog::join(std::slice::from_ref(acquired))
        .map_err(Error::other)
        .map_err(Proof::Failed)?;
    let application = catalog
        .prove_call(call, &graph.rust_edition())
        .map_err(|rejection| Proof::Rejected(rejection.to_string()))?;
    if applications
        .iter()
        .any(|known: &rawr_import::surface::application::Application| {
            known.call().key() == call.key()
        })
    {
        return Err(Proof::Failed(Error::other(
            "a proved imported call was not admitted by its compilation",
        )));
    }
    applications.push(application);
    Ok(())
}

/// Resolve, extract, check and seal a program's runner with no host profile.
fn prepare(
    target: &rawr::project::Target,
    graph: Option<rawr::package::Graph>,
    revision: rawr_import::supervisor::Revision,
    scope: &rawr_import::supervisor::snapshot::attempt::Scope,
) -> Result<crate::Prepared, crate::Error> {
    use crate::Error;
    let graph = match graph {
        Some(graph) => graph,
        None => rawr::package::Graph::load(&rawr::Engine::new(), target).map_err(Error::Rawr)?,
    };
    let limits = graph.rust_limits();
    let source = rawr::reload::source::Graph::from_package(&graph);
    let snapshot = rawr::reload::source::Snapshot::from_graph(&source);
    let expected = snapshot.imported(&limits).map_err(Error::other)?;
    let transport = snapshot
        .imported_transport(expected, &limits)
        .map_err(Error::other)?;
    let project = graph.entry_package().root().to_path_buf();
    let adapter = adapter(&limits).map_err(Error::other)?;
    let policy = rawr_import::classify::Policy::standard();
    let mut snapshot = Some(snapshot);
    let mut crate_demand = rawr_import::crate_demand::load(&project);
    let mut settlement = rawr_import::bindings::Settlement::default();
    let mut remembered = !crate_demand.is_empty();
    let seed = graph.rust_demand_seed(&[], None);
    // Proof-backed applications of imported calls whose candidate depends
    // on the call's receiver and lexical scope (D650), proved from the
    // dependencies' intrinsic facts, acquired once.
    let mut applications = Vec::new();
    let mut inventory = None;
    let (demand, profile_contract, cached, native) = loop {
        let mut request_demand = crate_demand.clone();
        request_demand.extend(seed.iter().cloned());
        request_demand.sort_unstable();
        request_demand.dedup();
        let environment = Environment::capture().map_err(Error::other)?;
        let universe =
            rawr_import::cargo::universe::Catalog::plain(&limits).map_err(Error::other)?;
        let request = match graph.rust_import_plain(environment, universe, &request_demand, &limits)
        {
            Ok(Some(request)) => request,
            Ok(None) => {
                return Err(Error::other(rawr_import::Error::Request {
                    field: "Rust imports",
                    reason: "the crate has no Rust dependency or native module".into(),
                }));
            }
            Err(_) if remembered => {
                remembered = false;
                crate_demand.clear();
                rawr_import::crate_demand::store(&project, &crate_demand);
                continue;
            }
            Err(error) => return Err(Error::other(error)),
        };
        remembered = false;
        let mut prepared = request
            .prepare_cached_in(&limits, scope)
            .map_err(Error::other)?;
        settlement.absorb(prepared.settlement());
        let api = prepared.retain_api(&limits).map_err(Error::other)?;
        let snapshot = snapshot
            .take()
            .unwrap_or_else(|| rawr::reload::source::Snapshot::from_graph(&source));
        let warm = rawr_import::composition::resolve_api_semantic(
            api,
            &adapter,
            &policy,
            expected,
            &limits,
            |facade| rawr::package::import::Brand::replay(facade).map_err(Error::other),
        );
        let warm = match warm {
            Ok(warm) => warm,
            Err(rawr_import::checked::Evaluation::Transform(error)) => return Err(error),
            Err(rawr_import::checked::Evaluation::Cache(error)) => return Err(Error::other(error)),
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
                    |contract, _profile, _facade| {
                        // Async code compiles against the core's future
                        // (D513.3), whose facade the runner installs too.
                        let demand = rawr::future::facade::engine()
                            .compile_imported_with_applications(contract, snapshot, &applications)
                            .map_err(Error::Rawr)?;
                        let checked = Checked(*demand.contract_fingerprint().as_bytes());
                        Ok((demand, checked))
                    },
                );
                let completed = match completed {
                    Ok(completed) => completed,
                    Err(rawr_import::checked::Evaluation::Transform(Error::Rawr(error)))
                        if error.rust_application().is_some() =>
                    {
                        let call = error
                            .rust_application()
                            .ok_or_else(|| Error::other("imported call encoding"))?;
                        prove(&graph, &limits, &call, &mut inventory, &mut applications).map_err(
                            |reason| match reason {
                                Proof::Rejected(reason) => {
                                    Error::other(format!("{}\n  {reason}", Error::Rawr(error)))
                                }
                                Proof::Failed(failure) => failure,
                            },
                        )?;
                        continue;
                    }
                    Err(rawr_import::checked::Evaluation::Transform(Error::Rawr(error)))
                        if crate::new_demand(&error, &request_demand).is_some() =>
                    {
                        crate_demand
                            .extend(crate::new_demand(&error, &request_demand).map(Box::from));
                        // A member a trait provides arrives only when named:
                        // each method the source calls is demanded on the
                        // type at once, as a builder chain names several.
                        if let Some(owner) = error.rust_member_owner() {
                            crate_demand.extend(
                                graph
                                    .rust_method_names()
                                    .iter()
                                    .map(|name| Box::from(format!("{owner}::{name}"))),
                            );
                        }
                        crate_demand.sort_unstable();
                        crate_demand.dedup();
                        rawr_import::crate_demand::store(&project, &crate_demand);
                        continue;
                    }
                    Err(rawr_import::checked::Evaluation::Transform(error)) => return Err(error),
                    Err(rawr_import::checked::Evaluation::Cache(error)) => {
                        return Err(Error::other(error));
                    }
                };
                let rawr_import::composition::Completed {
                    contract,
                    profile: profile_contract,
                    facade,
                    checked: cached,
                } = completed;
                let brand = rawr::package::import::Brand::replay(&facade).map_err(Error::other)?;
                let demand = cached
                    .replay_demand(brand, &contract, expected, &limits)
                    .map_err(Error::other)?
                    .with_trusted_runner(true, &facade, &contract)
                    .map_err(Error::other)?;
                rawr_import::composition::record_resolved(&contract, &demand, &adapter, &policy)
                    .map_err(Error::other)?;
                (
                    demand,
                    profile_contract,
                    cached,
                    contract.into_native_proof(),
                )
            }
        };
    };
    let profile = rawr_import::composition::profile::prepare_retained(
        &adapter,
        &profile_contract,
        &cached,
        &limits,
    )
    .map_err(Error::other)?;
    let request = demand
        .request_retained(native, profile, revision)
        .map_err(Error::other)?;
    let inputs = rawr::reload::source::Inputs::from_graph(&source).with_imported(request.watch());
    Ok(
        crate::Prepared::new(request, transport, project, inputs, &graph, limits)
            .with_settlement(settlement),
    )
}

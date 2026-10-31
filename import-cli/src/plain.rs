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

/// The plain profile. A program whose imported calls were proved from
/// intrinsic facts (D650) is checked under those facts too: the union's
/// identity joins the profile's, so a check cached under other facts is
/// never replayed for it.
fn adapter(
    limits: &rawr_import::cargo::Limits,
    facts: Option<&[u8; 32]>,
) -> Result<trusted::Adapter<Operations>, profile::Error> {
    let identity = match facts {
        Some(facts) => {
            let hex = facts
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            format!("{IDENTITY}.facts.{hex}")
        }
        None => IDENTITY.to_owned(),
    };
    Ok(trusted::Adapter::trusted(
        Operations,
        profile::Identity::new(&identity, limits)?,
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

/// Demand the Rust path a failure missed. A member a trait provides arrives
/// only when named: each method the source calls is demanded on the type
/// at once, as a builder chain names several.
fn demand_more(
    error: &rawr::package::Error,
    request_demand: &[Box<str>],
    crate_demand: &mut Vec<Box<str>>,
    graph: &rawr::package::Graph,
) {
    crate_demand.extend(crate::new_demand(error, request_demand).map(Box::from));
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
}

/// Record that a program's check depends on proved applications.
fn mark(applied: &std::path::Path) {
    if applied.is_file() {
        return;
    }
    if let Some(parent) = applied.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(applied, b"");
}

/// Why an imported call has no application.
enum Proof {
    /// Rust rejects the call, finds it ambiguous, or the closed rules
    /// cannot decide it.
    Rejected(String),
    /// Acquiring or joining the facts failed.
    Failed(crate::Error),
}

/// The dependency closure's intrinsic entries this preparation proves
/// with, and the aliases the program declares their crates by.
struct Facts {
    entries: Vec<rawr_import::applicability::inventory::Entry>,
    aliases: Vec<(Box<str>, Box<str>)>,
}

/// Each closure crate's entry from the project's inventory store, under the
/// key its native input witness sets before any nightly build. A crate
/// whose entry is absent or stale is acquired on the nightly, and its entry
/// published when the nightly saw the inputs the project's build sees.
fn acquire(
    graph: &rawr::package::Graph,
    limits: &rawr_import::cargo::Limits,
    input: &[u8; 32],
    settlement: &mut rawr_import::bindings::Settlement,
) -> Result<Facts, crate::Error> {
    use crate::Error;
    use rawr_import::applicability::{inventory, store};
    use std::path::PathBuf;
    // A crate whose whole closure comes from registries and git commits has
    // a machine-wide key, so its entry lives in rawr's home (D608.4). Any
    // other crate is keyed by its native input witness, the project's view
    // of it, so its entry stays with the project, as its build does.
    let local = graph.entry_package().root().join("target/rawr/inventories");
    // The entries a union of these exact API inputs was made of: read back
    // with no Cargo or nightly process (D609.6).
    let hex = |bytes: &[u8]| {
        bytes
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>()
    };
    let union = local.join(format!("union-{}.json", hex(input)));
    if let Ok(bytes) = std::fs::read(&union)
        && let Ok((aliases, listed)) =
            serde_json::from_slice::<(Vec<(Box<str>, Box<str>)>, Vec<([u8; 32], PathBuf)>)>(&bytes)
    {
        let entries = listed
            .iter()
            .map(|(key, directory)| store::read(directory, key).map(store::Held::into_entry))
            .collect::<Option<Vec<_>>>();
        if let Some(entries) = entries {
            return Ok(Facts { entries, aliases });
        }
    }
    let request = |closure: &[rawr_import::cargo::config::Dependency]| {
        let environment = Environment::capture().map_err(Error::other)?;
        graph
            .rust_inventory_plain(environment, closure, limits)
            .map_err(Error::other)?
            .ok_or_else(|| Error::other("the crate has no Rust dependency"))
    };
    let probe = request(&[])?;
    let native = probe.pinned_native(limits).map_err(Error::other)?;
    let closure = probe.closure(&native, limits).map_err(Error::other)?;
    // What the project's last build gave its build scripts, against which
    // an entry whose closure runs one is shared (#403).
    let rawr = graph.entry_package().root().join("target/rawr");
    let project = store::Scripts::project(&rawr);
    // Each unit: its entry's key and store, and the witness its capture must
    // agree with before it is published. A unit whose shared closure runs a
    // build script reads rawr's home unless the project's last build gave
    // other outputs than its entry was captured with, and this run's build
    // checks it (`settlement`). Otherwise its entry is the project's, and
    // goes to the home once the build gives the outputs the nightly did.
    let mut promote = std::collections::BTreeMap::new();
    let mut keys = std::collections::BTreeMap::new();
    let mut entries = Vec::new();
    let mut missing = std::collections::BTreeSet::new();
    let mut complete = true;
    for (name, unit, witness) in &closure.units {
        let scripts = closure.scripts.get(unit);
        let (key, directory, shared) = match closure.shared.get(unit) {
            Some(shared) if scripts.is_none() => {
                (store::shared_key(name, shared), closure.home.clone(), true)
            }
            Some(shared) => {
                let packages = scripts.cloned().unwrap_or_default();
                let key = store::shared_key(name, shared);
                if let Some(held) = store::read(&closure.home, &key)
                    && project
                        .as_ref()
                        .is_none_or(|project| held.entry().scripts_agree(&packages, project))
                {
                    settlement.inventory_read(&rawr, &closure.home, &key, &packages, held.entry());
                    keys.insert(unit.clone(), (key, *witness, closure.home.clone(), true));
                    entries.push(held.into_entry());
                    continue;
                }
                promote.insert(unit.clone(), (key, packages));
                (store::key(name, witness), local.clone(), false)
            }
            None => (store::key(name, witness), local.clone(), false),
        };
        match store::read(&directory, &key) {
            Some(held) if shared || held.entry().native().1 == witness => {
                entries.push(held.into_entry());
            }
            _ => {
                missing.insert(key);
            }
        }
        keys.insert(unit.clone(), (key, *witness, directory, shared));
    }
    if !missing.is_empty() {
        let request = request(&closure.dependencies)?;
        let acquired =
            inventory::Acquired::capture(request, &native, limits).map_err(Error::other)?;
        entries = acquired
            .entries(|unit| keys.get(unit).map(|(key, ..)| *key))
            .map_err(Error::other)?;
        for entry in &entries {
            let (unit, witness) = entry.native();
            let Some((_, expected, directory, _)) = keys.get(unit) else {
                complete = false;
                continue;
            };
            // The nightly saw the inputs the project's build sees, or the
            // entry stays local to this run.
            let agrees = expected == witness;
            complete &= agrees;
            if missing.contains(entry.key()) && agrees {
                complete &= store::publish(directory, entry).is_ok();
            }
        }
    }
    // A project's entry whose build scripts give this run's build what they
    // gave the nightly goes to rawr's home, for any project whose build
    // agrees, once that build has run.
    for entry in &entries {
        if let Some((key, packages)) = promote.get(entry.native().0)
            && store::read(&closure.home, key).is_none()
        {
            settlement.inventory(&rawr, &closure.home, key, packages, entry);
        }
    }
    if complete {
        let listed = entries
            .iter()
            .filter_map(|entry| {
                let (_, _, directory, _) = keys.get(entry.native().0)?;
                Some((*entry.key(), directory.clone()))
            })
            .collect::<Vec<_>>();
        if listed.len() == entries.len()
            && let Ok(bytes) = serde_json::to_vec(&(&closure.aliases, listed))
        {
            let _ = std::fs::create_dir_all(&local);
            let staging = local.join(format!(".union-{}.{}", hex(input), std::process::id()));
            if std::fs::write(&staging, bytes).is_ok() {
                let _ = std::fs::rename(&staging, &union);
            }
        }
    }
    Ok(Facts {
        entries,
        aliases: closure.aliases,
    })
}

/// Prove one imported call from the dependency closure's intrinsic facts
/// and keep its application. No compiler runs here.
fn prove(
    graph: &rawr::package::Graph,
    facts: &Facts,
    call: &rawr_import::surface::application::Call,
    applications: &mut Vec<rawr_import::surface::application::Application>,
) -> Result<(), Proof> {
    use crate::Error;
    let catalog = rawr_import::applicability::Catalog::stored(&facts.entries, &facts.aliases)
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
    // A program that used proved applications last time keys its check by
    // the identity of every input its API was read from: its dependencies'
    // facts change only with them, so no facts are read for a warm run.
    let applied = project.join("target/rawr/inventories/applied");
    let mut inventory = None;
    let mut keyed;
    let mut current_adapter = None;
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
        let input = api.input_identity();
        keyed = applied.is_file().then_some(input);
        let adapter =
            &*current_adapter.insert(adapter(&limits, keyed.as_ref()).map_err(Error::other)?);
        let snapshot = snapshot
            .take()
            .unwrap_or_else(|| rawr::reload::source::Snapshot::from_graph(&source));
        let warm = rawr_import::composition::resolve_api_semantic(
            api,
            adapter,
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
                    adapter,
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
                        if inventory.is_none() {
                            inventory = Some(acquire(&graph, &limits, &input, &mut settlement)?);
                        }
                        let facts = inventory
                            .as_ref()
                            .ok_or_else(|| Error::other("intrinsic facts"))?;
                        match prove(&graph, facts, &call, &mut applications) {
                            Ok(()) => {
                                // The check now depends on proved facts:
                                // the next attempt is keyed by them already,
                                // so the next run replays it (#400).
                                mark(&applied);
                                continue;
                            }
                            Err(Proof::Failed(failure)) => return Err(failure),
                            // An unproved miss of a member the facade lacks
                            // is demanded as before; demanded already, it
                            // is reported as the miss it is.
                            Err(Proof::Rejected(_))
                                if crate::new_demand(&error, &request_demand).is_some() =>
                            {
                                demand_more(&error, &request_demand, &mut crate_demand, &graph);
                                rawr_import::crate_demand::store(&project, &crate_demand);
                                continue;
                            }
                            Err(Proof::Rejected(_)) if error.rust_member_owner().is_some() => {
                                return Err(Error::Rawr(error));
                            }
                            Err(Proof::Rejected(reason)) => {
                                return Err(Error::other(format!(
                                    "{}\n  {reason}",
                                    Error::Rawr(error)
                                )));
                            }
                        }
                    }
                    Err(rawr_import::checked::Evaluation::Transform(Error::Rawr(error)))
                        if crate::new_demand(&error, &request_demand).is_some() =>
                    {
                        demand_more(&error, &request_demand, &mut crate_demand, &graph);
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
                rawr_import::composition::record_resolved(&contract, &demand, adapter, &policy)
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
    // Remember whether this program's check depends on proved applications,
    // so its next run keys the check by the facts it then has.
    if demand.application_catalogs().is_empty() {
        let _ = std::fs::remove_file(&applied);
    } else if keyed.is_none() {
        mark(&applied);
    }
    let adapter = current_adapter.ok_or_else(|| Error::other("the plain profile adapter"))?;
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

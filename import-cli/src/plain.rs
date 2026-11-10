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

/// How many rounds this process's checks ran: each reads the dependencies'
/// API once and checks the program against it.
static ROUNDS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// How many rounds this process's checks ran.
pub(crate) fn rounds() -> usize {
    ROUNDS.load(std::sync::atomic::Ordering::Relaxed)
}

/// The plain profile. A program whose imported calls were proved from
/// intrinsic facts (D650) is checked under those facts too: the union's
/// identity joins the profile's, so a check cached under other facts is
/// never replayed for it.
fn adapter(
    facts: Option<&[u8; 32]>,
    instances: &rawr_import::record::Record,
) -> trusted::Adapter<Operations> {
    let mut identity = match facts {
        Some(facts) => {
            let hex = facts
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            format!("{IDENTITY}.facts.{hex}")
        }
        None => IDENTITY.to_owned(),
    };
    // A check compiles each instance native code calls a marked generic at
    // (D869): one cached for other instances is never replayed.
    if !instances.instances().is_empty() {
        let key = instances
            .instances()
            .iter()
            .map(|instance| format!("{}\0{}", instance.path, instance.name))
            .collect::<Vec<_>>()
            .join("\n");
        identity.push_str(".instances.");
        identity.push_str(&rawr::package::instances::digest(&key));
    }
    trusted::Adapter::trusted(
        Operations,
        profile::Identity::new(&identity),
        profile::Version::new(VERSION),
        false,
    )
    .semantic_api()
}

/// The profile that adds nothing to the runner.
#[derive(Clone, Copy, Debug, Default)]
pub struct Plain;

impl crate::Profile for Plain {
    fn prepare(
        &self,
        target: &rawr::project::Target,
        mut graph: Option<rawr::package::Graph>,
        revision: rawr_import::supervisor::Revision,
        scope: &rawr_import::supervisor::snapshot::attempt::Scope,
    ) -> Result<crate::Prepared, crate::Error> {
        crate::again_while_stale(|| prepare(target, graph.take(), revision, scope))
    }
}

/// Demand every Rust path a failure missed. A member a trait provides arrives
/// only when named: each method the source calls is demanded on the type
/// at once, as a builder chain names several.
fn demand_more(
    error: &rawr::package::Error,
    request_demand: &[Box<str>],
    crate_demand: &mut Vec<Box<str>>,
    graph: &rawr::package::Graph,
) {
    // Every miss of the failed pass, its own first (#662). A member called
    // on a type that was missed too asks the crate's compiler which trait
    // gives it: only worth asking when another member already makes the
    // next round probe, since the type alone replays the tables it has.
    let misses = error.rust_misses();
    let missed_type = |owner: &str| misses.iter().any(|(path, ..)| *path == owner);
    let probing = misses
        .iter()
        .any(|(_, _, owner)| owner.is_some_and(|owner| !missed_type(owner)));
    for &(path, through_alias, owner) in &misses {
        if owner.is_some_and(missed_type) && !probing {
            continue;
        }
        crate_demand.extend(crate::demandable(path, through_alias, request_demand).map(Box::from));
        // A member the type lacks may be its `Deref` target's, which the
        // method probe reaches through its `deref` and `deref_mut` (#762).
        // Only a crate's type is extracted: std's `String` is no crate.
        let root = |path: &str| path.split("::").next().map(str::to_owned);
        if let Some(owner) = owner
            && (through_alias
                || request_demand
                    .iter()
                    .any(|known| root(known) == root(owner)))
        {
            crate_demand.extend(
                graph
                    .rust_method_names()
                    .iter()
                    .map(|name| &**name)
                    .chain(["deref", "deref_mut"])
                    .map(|name| Box::from(format!("{owner}::{name}"))),
            );
        }
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
        entries = acquired.entries(|unit| keys.get(unit).map(|(key, ..)| *key));
        for entry in &entries {
            let (unit, witness) = entry.native();
            // `entries` keyed every entry from `keys`.
            let (_, expected, directory, _) = &keys[unit];
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
            "the check rejected an imported call it was already given",
        )));
    }
    applications.push(application);
    Ok(())
}

/// Expand through rustc each crate derive `error` asks for, keeping each
/// expansion where loading the program reads it beside its item (#593).
/// `basis` is what the run's own resolution built, when it built one.
/// `false` when each is kept already, so loading again can't help.
fn expand(
    graph: &rawr::package::Graph,
    error: &rawr::package::Error,
    basis: Option<rawr_import::expand::Basis>,
    limits: &rawr_import::cargo::Limits,
) -> Result<bool, crate::Error> {
    use rawr_import::expand;
    let project = graph.entry_package().root();
    let pending = error
        .rust_expansions()
        .into_iter()
        .filter(|item| {
            item.derives.iter().any(|(written, resolved)| {
                let kept =
                    std::fs::read_to_string(expand::source_path(project, &item.path, written));
                kept.ok()
                    .as_deref()
                    .and_then(|text| expand::kept(text, &item.text))
                    != Some(expand::Kept::Current(resolved.as_str()))
            })
        })
        .collect::<Vec<_>>();
    if pending.is_empty() {
        return Ok(false);
    }
    keep_expansions(graph, &pending, basis, limits)?;
    Ok(true)
}

/// Expand again each item of `graph` whose kept expansion is of its text
/// before an edit (#1040), against the basis the last expansion kept, so the
/// edit's expansion changes its source in place. `false` when there is none,
/// or no basis is kept: preparing the program expands it then.
pub fn refresh_expansions(graph: &rawr::package::Graph) -> Result<bool, crate::Error> {
    let stale = graph.stale_expansions();
    if stale.is_empty() {
        return Ok(false);
    }
    let directory = rawr_import::expand::directory(graph.entry_package().root());
    let Some(basis) = rawr_import::expand::Basis::kept(&directory) else {
        return Ok(false);
    };
    keep_expansions(graph, stale, Some(basis), &graph.rust_limits())?;
    Ok(true)
}

/// rustc's expansion of each of `items`, in one driver run, kept as its
/// source beside its item. The basis is `basis`, else the one the last
/// expansion kept, else a resolution's of its own, which is kept for the
/// next.
fn keep_expansions(
    graph: &rawr::package::Graph,
    items: &[rawr_import::expand::Item],
    basis: Option<rawr_import::expand::Basis>,
    limits: &rawr_import::cargo::Limits,
) -> Result<(), crate::Error> {
    use crate::Error;
    use rawr_import::expand;
    let project = graph.entry_package().root();
    let directory = expand::directory(project);
    let environment = Environment::capture().map_err(Error::other)?;
    let request = graph
        .rust_inventory_plain(environment, &[], limits)
        .map_err(Error::other)?
        .ok_or_else(|| Error::other("cannot expand a derive in a crate with no Rust dependency"))?;
    let native = request.pinned_native(limits).map_err(Error::other)?;
    let basis = match basis.or_else(|| expand::Basis::kept(&directory)) {
        Some(basis) => basis,
        None => expand::Basis::capture(request, &native, limits).map_err(Error::other)?,
    };
    basis.keep(&directory).map_err(Error::other)?;
    let edition = graph.rust_edition();
    let expanded = expand::expand(items, &basis, &edition, &directory, &native, limits)
        .map_err(Error::other)?;
    for (item, expanded) in items.iter().zip(&expanded) {
        expand::keep_sources(project, item, expanded).map_err(Error::other)?;
    }
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
    let seed = graph.rust_demand_seed();
    // Proof-backed applications of imported calls whose candidate depends
    // on the call's receiver and lexical scope (D650), proved from the
    // dependencies' intrinsic facts, acquired once.
    let mut applications = Vec::new();
    // Whether the check ran again for a new record of native instances.
    let mut instances_retried = false;
    // Why the call a check stopped at has no application, when the check
    // tried to prove it in place.
    let mut unproved = None;
    // A cache entry that goes stale while this run uses it is `Error::Stale`,
    // and `Plain::prepare` starts the run again, whose lookup rebuilds it.
    let (demand, profile_contract, cached, native) = loop {
        ROUNDS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut request_demand = crate_demand.clone();
        request_demand.extend(seed.iter().cloned());
        // The record a retry stored names types of its own (D869).
        if instances_retried {
            request_demand.extend(graph.rust_demand_seed());
        }
        request_demand.sort_unstable();
        request_demand.dedup();
        let environment = Environment::capture().map_err(Error::other)?;
        let universe = rawr_import::cargo::universe::Catalog::plain().map_err(Error::other)?;
        let request = match graph.rust_import_plain(environment, universe, &request_demand, &limits)
        {
            Ok(Some(request)) => request,
            Ok(None) => {
                return Err(Error::other(rawr_import::Error::Request {
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
        // A first build whose check records instances the table lacks stops
        // before its probe: its API is read once, after the retry.
        let request = if instances_retried {
            request
        } else {
            request.kept_instances(rawr::package::instances::kept(&project))
        };
        rawr_import::progress::phase("resolving");
        let mut prepared = request
            .prepare_cached_in(&limits, scope)
            .map_err(Error::import)?;
        settlement.absorb(prepared.settlement());
        // The instances native code calls marked generics at, as this check
        // recorded them (D866): a record the shadow crate's table doesn't
        // hold yet is written, and the crate checked again with it (D869).
        let record = prepared.recorded_instances().map_err(Error::other)?;
        if let Some(record) = &record
            && rawr::package::instances::store(&project, record).map_err(Error::other)?
            && !rawr::package::instances::kept(&project)
                .instances()
                .is_empty()
            && !instances_retried
        {
            instances_retried = true;
            continue;
        }
        // What a crate derive's expansion runs against: this run's own build.
        let basis = prepared.expansion_basis();
        rawr_import::progress::phase("reading API");
        let api = prepared.retain_api(&limits).map_err(Error::import)?;
        let input = api.input_identity();
        keyed = applied.is_file().then_some(input);
        let adapter = &*current_adapter.insert(adapter(
            keyed.as_ref(),
            &rawr::package::instances::kept(&project),
        ));
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
            Err(rawr_import::checked::Evaluation::Cache(error)) => return Err(Error::cache(error)),
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
                        let mut snapshot = Some(snapshot);
                        loop {
                            let snapshot = snapshot.take().unwrap_or_else(|| {
                                rawr::reload::source::Snapshot::from_graph(&source)
                            });
                            // Async code compiles against the core's future
                            // (D513.3), whose facade the runner installs
                            // too. `rawr run` and `rawr check` read the
                            // program as Cargo builds it: with everything
                            // (D683.4).
                            let mut engine = rawr::future::facade::engine();
                            engine.set_access(rawr::Access::all());
                            let error = match engine.compile_imported_with_applications(
                                contract,
                                snapshot,
                                &applications,
                            ) {
                                Ok(demand) => {
                                    let checked =
                                        Checked(*demand.contract_fingerprint().as_bytes());
                                    return Ok((demand, checked));
                                }
                                Err(error) => error,
                            };
                            // A check keyed by the crates' facts already
                            // proves each call it needs in place: a proof
                            // changes only the check (#446). The first
                            // proof of an unkeyed check starts it again
                            // under the facts' key, below.
                            let (Some(_), Some(call)) = (keyed, error.rust_application()) else {
                                return Err(Error::Rawr(error));
                            };
                            if inventory.is_none() {
                                inventory =
                                    Some(acquire(&graph, &limits, &input, &mut settlement)?);
                            }
                            let facts = inventory
                                .as_ref()
                                .ok_or_else(|| Error::other("the crates' facts were not loaded"))?;
                            if let Err(proof) = prove(&graph, facts, &call, &mut applications) {
                                unproved = Some(proof);
                                return Err(Error::Rawr(error));
                            }
                        }
                    },
                );
                let completed = match completed {
                    Ok(completed) => completed,
                    // rustc expands a crate's derive on a program type, and
                    // the expansion is program code (D740): kept beside its
                    // item, the next load reads it.
                    Err(rawr_import::checked::Evaluation::Transform(Error::Rawr(error)))
                        if !error.rust_expansions().is_empty() =>
                    {
                        if !expand(&graph, &error, basis, &limits)? {
                            return Err(Error::Rawr(error));
                        }
                        return prepare(target, None, revision, scope);
                    }
                    Err(rawr_import::checked::Evaluation::Transform(Error::Rawr(error)))
                        if error.rust_application().is_some() =>
                    {
                        let proved = match unproved.take() {
                            Some(proof) => Err(proof),
                            None => {
                                let call = error.rust_application().ok_or_else(|| {
                                    Error::other("the failed check names no imported call")
                                })?;
                                if inventory.is_none() {
                                    inventory =
                                        Some(acquire(&graph, &limits, &input, &mut settlement)?);
                                }
                                let facts = inventory.as_ref().ok_or_else(|| {
                                    Error::other("the crates' facts were not loaded")
                                })?;
                                prove(&graph, facts, &call, &mut applications)
                            }
                        };
                        match proved {
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
                        return Err(Error::cache(error));
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
                    .map_err(Error::cache)?
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
    let adapter =
        current_adapter.ok_or_else(|| Error::other("no adapter was prepared for the program"))?;
    let profile =
        rawr_import::composition::profile::prepare_retained(&adapter, &profile_contract, &cached)
            .map_err(Error::cache)?;
    let request = demand
        .request_retained(native, profile, revision)
        .map_err(Error::cache)?;
    let outputs = rawr_import::location::outputs_here(&project);
    let native = rawr::reload::source::Inputs::imported(request.watch())
        .outside(&outputs)
        .with_checkouts(&rawr_import::location::checkouts_here());
    let inputs = rawr::reload::source::Inputs::from_graph(&source).with_native(&native);
    Ok(
        crate::Prepared::new(request, transport, project, inputs, &graph, limits)
            .with_settlement(settlement),
    )
}

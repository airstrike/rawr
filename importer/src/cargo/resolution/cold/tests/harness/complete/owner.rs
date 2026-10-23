//! Real semantic-node publication and replay over the complete cold fixture.

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::*;
use crate::cargo::resolution::owner;
use crate::node;

thread_local! {
    static ATTEMPT: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    /// Registry packages reachable from the protected unit in this thread's fixture.
    static REGISTRY: Cell<usize> = const { Cell::new(0) };
    /// Final-form lock bytes the last script wrote.
    static LOCK_BYTES: Cell<usize> = const { Cell::new(0) };
    /// Charge the warm graph exactly to its path and decode targets before
    /// it starts, so every warm charge is an exceedance.
    static PRECHARGE_WARM: Cell<bool> = const { Cell::new(false) };
    /// Open the next owner fixture at this exact (removed) root, so two
    /// limits profiles run over byte-identical paths.
    static FIXTURE_ROOT: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

/// Gallery's prior harness lock holds 345 packages, 338 of them from crates.io,
/// in 85,521 bytes; this many synthetic registry packages renders about that.
const GALLERY_REGISTRY: usize = 340;

/// Per-graph account figures for one owner graph: request high water
/// plus the graph's decode, path-check and hashed high waters and the axes that
/// crossed their targets.
#[derive(Clone, Copy)]
struct GraphFigures {
    request: usize,
    decoded: u64,
    paths: usize,
    hashed: u64,
    exceeded: crate::exceedance::Record,
}

impl GraphFigures {
    fn capture(request: usize, graph: &node::GraphBudget) -> Self {
        let (decoded, paths, hashed) = graph.census_usage().expect("graph census");
        Self {
            request,
            decoded,
            paths,
            hashed,
            exceeded: graph.exceedances(),
        }
    }

    fn report(&self, name: &str, limits: &Limits) {
        eprintln!(
            "  {name}: request high water {} of {} bytes; decode {} of {} bytes; path checks {} of \
             {}; hashed {} bytes",
            self.request,
            limits.manifest_bytes,
            self.decoded,
            limits.graph_targets().decoded,
            self.paths,
            limits.graph_targets().paths,
            self.hashed,
        );
        if self.exceeded.is_exceeded() {
            for axis in self.exceeded.axes() {
                eprintln!(
                    "    exceeded: {} reached {} (target {}){}",
                    axis.name,
                    axis.high_water,
                    axis.target,
                    axis.site
                        .map(|site| format!(" first at {}:{}", site.file(), site.line()))
                        .unwrap_or_default()
                );
            }
        } else {
            eprintln!("    no accounting target exceeded");
        }
    }
}

/// Request-account high water per owner graph, in bytes, with per-graph
/// account figures and the cold graph's semantic output for cross-target
/// comparison.
struct Measured {
    lock: usize,
    cold: usize,
    repair: usize,
    warm: usize,
    cold_graph: GraphFigures,
    repair_graph: GraphFigures,
    warm_graph: GraphFigures,
    semantic: [[u8; 32]; 8],
    contents: [node::Digest; 6],
    /// The cold graph's complete generated output: every generated file's
    /// exact bytes, by path.
    files: Vec<(Box<str>, Box<[u8]>)>,
    /// The cold graph's checked product content.
    checked: Box<[u8]>,
    /// The fixture root the cohort ran under (removed when it returns).
    root: PathBuf,
}

fn high_water() -> usize {
    cargo::REQUEST_HIGH_WATER.with(Cell::get)
}

/// Print this thread's graph decode charges by call site, largest first.
fn report_decode_sites() {
    let mut sites: Vec<_> = node::DECODE_SITES.with(|sites| {
        sites
            .borrow()
            .iter()
            .map(|(site, (calls, bytes))| (site.clone(), *calls, *bytes))
            .collect()
    });
    sites.sort_by_key(|(_, _, bytes)| std::cmp::Reverse(*bytes));
    let total: u64 = sites.iter().map(|(_, _, bytes)| bytes).sum();
    eprintln!(
        "gallery-scale graph decode charges: {total} bytes over {} sites",
        sites.len()
    );
    for (site, calls, bytes) in sites.iter().take(20) {
        eprintln!("  {bytes:>12} bytes {calls:>7} calls  {site}");
    }
    let mut files = BTreeMap::<&str, (usize, u64)>::new();
    for (site, calls, bytes) in &sites {
        let file = site
            .rsplit_once(':')
            .map_or(site.as_str(), |(file, _)| file);
        let entry = files.entry(file).or_default();
        entry.0 += calls;
        entry.1 += bytes;
    }
    let mut files: Vec<_> = files.into_iter().collect();
    files.sort_by_key(|(_, (_, bytes))| std::cmp::Reverse(*bytes));
    eprintln!("gallery-scale graph decode charges by file:");
    for (file, (calls, bytes)) in files {
        eprintln!("  {bytes:>12} bytes {calls:>7} calls  {file}");
    }
    owner::REPLAY_TRACE.with(|trace| {
        for (index, (decoded, files)) in trace.borrow().iter().enumerate() {
            let mut files: Vec<_> = files.iter().filter(|(_, bytes)| **bytes > 0).collect();
            files.sort_by_key(|(_, bytes)| std::cmp::Reverse(**bytes));
            let top: Vec<String> = files
                .iter()
                .take(6)
                .map(|(file, bytes)| format!("{file}={bytes}"))
                .collect();
            eprintln!("  validate {index}: +{decoded} decoded; {}", top.join(" "));
        }
    });
    let mut paths: Vec<_> = node::PATH_SITES.with(|sites| {
        sites
            .borrow()
            .iter()
            .map(|(site, (calls, checks))| (site.clone(), *calls, *checks))
            .collect()
    });
    paths.sort_by_key(|(_, _, checks)| std::cmp::Reverse(*checks));
    let total: u64 = paths.iter().map(|(_, _, checks)| checks).sum();
    eprintln!(
        "gallery-scale graph path checks: {total} of {} over {} sites",
        node::MAX_PATH_CHECKS_PER_GRAPH,
        paths.len()
    );
    for (site, calls, checks) in paths.iter().take(20) {
        eprintln!("  {checks:>9} checks {calls:>7} calls  {site}");
    }
}

fn reset_high_water() {
    cargo::REQUEST_HIGH_WATER.with(|high| high.set(0));
}

fn script(
    request: &cargo::Request,
    cached: &cargo::CachedToolchain,
    layout: &Layout,
) -> test_launch::Script {
    ATTEMPT.with(|slot| *slot.borrow_mut() = Some(layout.root().to_path_buf()));
    let native = process::Native::from_attestation(&cached.attestation);
    let (metadata, units) = documents_scaled(request, layout.final_form(), REGISTRY.get());
    let lock = render_lock(&metadata, LockScope::Final);
    LOCK_BYTES.set(lock.len());
    eprintln!(
        "gallery-scale documents: metadata {} bytes, units {} bytes, lock {} bytes, root {}",
        metadata.len(),
        units.len(),
        lock.len(),
        layout.root().display()
    );
    let mut steps = super::super::built::fresh_steps_with_lock(
        request,
        &native,
        layout,
        &metadata,
        &units,
        Some(lock),
    );
    let (build_bytes, files) = artifacts(layout, &metadata, &request.target);
    steps.push(expected_reply(
        request,
        &native,
        layout.final_form(),
        Command::Build,
        true,
        move |_| test_launch::Reply::exit(0, build_bytes, Vec::new(), files),
    ));
    steps.push(probe(layout, &native, &cached.context, Outcome::Success));
    test_launch::Script::new(
        layout.root().to_path_buf(),
        // Gallery-scale replies write two artifact files per package plus the
        // metadata, units and lock documents.
        test_launch::FileLimits {
            count: 1024,
            bytes: 4 * 1024 * 1024,
        },
        steps,
    )
    .expect("complete owner cold script")
}

fn no_cold(_: &cargo::Request, _: &cargo::CachedToolchain, _: &Layout) -> test_launch::Script {
    panic!("an unchanged current owner must not enter mutable preparation")
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, (Vec<u8>, std::time::SystemTime, bool)> {
    let mut pending = vec![root.to_path_buf()];
    let mut files = BTreeMap::new();
    while let Some(path) = pending.pop() {
        let metadata = fs::symlink_metadata(&path).unwrap();
        assert!(!metadata.file_type().is_symlink());
        if metadata.is_dir() {
            pending.extend(
                fs::read_dir(path)
                    .unwrap()
                    .map(|entry| entry.unwrap().path()),
            );
        } else {
            files.insert(
                path.strip_prefix(root).unwrap().to_path_buf(),
                (
                    fs::read(&path).unwrap(),
                    metadata.modified().unwrap(),
                    metadata.permissions().readonly(),
                ),
            );
        }
    }
    files
}

#[test]
fn complete_owner_node_reuses_without_children_or_mutating_bundle() {
    let Fixture {
        root,
        mut request,
        context,
        store,
    } = Fixture::open();
    let limits = Limits::standard();
    let cached = || cargo::CachedToolchain {
        context: context.clone(),
        attestation: context.cached_toolchain().unwrap(),
    };
    // Seed authentication describes protected sources and the fixed command
    // cohort, independent of the actual owner's later Attempt namespace.
    Fixture::reserve_parts(&context, &store, |_, _| {
        bind_universe(&mut request, &cached());
        Ok(())
    });
    let universe = request.universe.clone();
    let cold_metrics = crate::metrics::recorder::Recorder::transform(1000).unwrap();
    let scope = cold_metrics.scope().unwrap();
    let cold =
        owner::evaluate_scripted(request, cached(), &limits, &crate::metrics::Current, script)
            .expect("actual semantic node cold publication");
    let cold_identity = *cold.node().value().completed_identity();
    let cold_action = cold.node().receipt().action();
    let cold_content = cold.node().content().digest();
    #[cfg(feature = "import-baseline")]
    let cold_observation = (cold.observed_toolchain(), cold.observed_identity());
    let cold_api = cold
        .publish(&limits)
        .expect("current owner publishes the real API");
    let api_identity = *cold_api.fingerprint().as_bytes();
    drop(scope);
    let cold_metrics = cold_metrics.snapshot().unwrap();
    assert_eq!(
        api_count(&cold_metrics, crate::metrics::NodeEvent::ExecutedMiss),
        1
    );
    let bundle = ATTEMPT.with(|slot| slot.borrow_mut().take().unwrap());
    let before = snapshot(&bundle);
    let mut request = Fixture::request(&root, context.environment().clone());
    request.universe = universe.clone();
    let warm_metrics = crate::metrics::recorder::Recorder::transform(1000).unwrap();
    let scope = warm_metrics.scope().unwrap();
    let warm_api = test_launch::Script::new(
        root.clone(),
        test_launch::FileLimits { count: 0, bytes: 0 },
        Vec::new(),
    )
    .unwrap()
    .run(|| {
        let warm = owner::evaluate_scripted(
            request,
            cached(),
            &limits,
            &crate::metrics::Current,
            no_cold,
        )
        .expect("warm semantic node validates");
        assert_eq!(warm.node().value().completed_identity(), &cold_identity);
        assert_eq!(warm.node().receipt().action(), cold_action);
        assert_eq!(warm.node().content().digest(), cold_content);
        #[cfg(feature = "import-baseline")]
        assert_eq!(
            (warm.observed_toolchain(), warm.observed_identity()),
            cold_observation
        );
        warm.publish(&limits)
    })
    .expect("warm owner and API launch no child")
    .expect("warm API publishes");
    drop(scope);
    let warm_metrics = warm_metrics.snapshot().unwrap();
    assert_eq!(
        api_count(&warm_metrics, crate::metrics::NodeEvent::ExecutedMiss),
        0
    );
    assert_eq!(
        api_count(&warm_metrics, crate::metrics::NodeEvent::ValidatedHit),
        1
    );
    assert_eq!(warm_api.fingerprint().as_bytes(), &api_identity);
    assert_eq!(
        snapshot(&bundle),
        before,
        "warm owner does not rewrite sealed files or permissions"
    );

    let mut request = Fixture::request(&root, context.environment().clone());
    request.universe = universe.clone();
    let pending = owner::evaluate_scripted(
        request,
        cached(),
        &limits,
        &crate::metrics::Current,
        no_cold,
    )
    .unwrap();
    let source = root.join("authority/.cargo/config.toml");
    let original = fs::read(&source).unwrap();
    fs::write(&source, b"[net]\noffline = false\n").unwrap();
    assert!(
        pending.publish(&limits).is_err(),
        "API hit cannot promote an owner held across source changes"
    );
    fs::write(&source, original).unwrap();

    let mut request = Fixture::request(&root, context.environment().clone());
    request.universe = universe.clone();
    let pending = owner::evaluate_scripted(
        request,
        cached(),
        &limits,
        &crate::metrics::Current,
        no_cold,
    )
    .unwrap();
    let attestation = cached();
    let executable = attestation
        .attestation
        .value()
        .proof()
        .executables()
        .find(|(name, _, _, _)| *name == "cargo")
        .unwrap()
        .1
        .to_path_buf();
    assert!(
        executable.starts_with(&root),
        "only mutate the fixture executable"
    );
    let original = fs::read(&executable).unwrap();
    fs::write(&executable, b"changed fixture executable").unwrap();
    assert!(
        pending.publish(&limits).is_err(),
        "API publication must refresh transitive executable authority"
    );
    fs::write(&executable, original).unwrap();

    let mut request = Fixture::request(&root, context.environment().clone());
    request.universe = universe;
    let foreign = owner::evaluate_scripted(
        request,
        cached(),
        &limits,
        &crate::metrics::Current,
        no_cold,
    )
    .unwrap();
    assert!(
        foreign.publish_input(&node::GraphBudget::new()).is_err(),
        "foreign graph cannot consume completed inputs"
    );
    drop((cold_api, warm_api));
    drop(context);
    store.retire_fixture().unwrap();
    fs::remove_dir_all(root).unwrap();
}

fn api_count(
    snapshot: &crate::metrics::recorder::Snapshot,
    event: crate::metrics::NodeEvent,
) -> u64 {
    snapshot
        .nodes
        .iter()
        .find(|count| count.kind == crate::metrics::NodeKind::Api && count.event == event)
        .unwrap()
        .count
}

fn api_continuation_fixture(limits: &Limits) -> Fixture {
    let root = FIXTURE_ROOT.with(|root| root.borrow_mut().take());
    let mut fixture = Fixture::open_at(limits, root.as_deref());
    let cached = cargo::CachedToolchain {
        context: fixture.context.clone(),
        attestation: fixture.context.cached_toolchain().unwrap(),
    };
    Fixture::reserve_parts(&fixture.context, &fixture.store, |layout, _| {
        let (metadata, _) = documents_scaled(&fixture.request, layout.final_form(), REGISTRY.get());
        bind_universe_seeded(&mut fixture.request, &cached, seed_lock(&metadata));
        Ok(())
    });
    fixture
}

#[cfg(unix)]
#[test]
fn protected_owner_rebuild_reuses_derived_semantic_suffix_and_successor() {
    protected_owner_rebuild(0, &Limits::standard());
}

/// Name the axes a cohort's three graphs crossed, for the completion report.
fn exceeded_axes(measured: &Measured) -> Vec<String> {
    let mut axes = Vec::new();
    for (graph, figures) in [
        ("cold", &measured.cold_graph),
        ("repair", &measured.repair_graph),
        ("warm", &measured.warm_graph),
    ] {
        for axis in figures.exceeded.axes() {
            axes.push(format!(
                "{graph}: {} reached {} (target {})",
                axis.name, axis.high_water, axis.target
            ));
        }
    }
    axes
}

/// The owner route (`prepare_cached_in`) captures its context in
/// the original attempt scope, whose graph account is bound to the captured
/// `[rust.limits]` graph targets rather than the standard ones; a path charge
/// past a configured `graph-path-checks = 1` is recorded against that target.
#[test]
fn owner_route_scope_measures_configured_graph_targets() {
    let mut limits = Limits::standard();
    limits.configure("graph-path-checks", 1).unwrap();
    let fixture = Fixture::open_with(&limits);
    let scope = crate::supervisor::snapshot::attempt::Scope::begin();
    let context = owner::scoped_context_for_test(&fixture.request, &limits, &scope)
        .expect("the owner route captures its context in the attempt scope");
    assert_eq!(context.graph_budget().targets(), limits.graph_targets());
    assert!(context.graph_budget().shares_with(scope.budget()));
    assert!(!scope.budget().exceedances().is_exceeded());
    context
        .graph_budget()
        .paths(2)
        .expect("an accounting axis never refuses");
    let record = scope.budget().exceedances();
    let axis = record
        .axis(node::PATHS_RESOURCE)
        .expect("the owner route records the configured path-check crossing");
    assert_eq!(
        (axis.target, axis.first_requested, axis.high_water),
        (1, 2, 2)
    );
    assert_eq!(record.axes().count(), 1);
    // The same scope keeps the same account and profile on a later route.
    let again = owner::scoped_context_for_test(&fixture.request, &limits, &scope).unwrap();
    assert!(again.graph_budget().shares_with(scope.budget()));
    assert_eq!(again.identity(), context.identity());
    // The configured profile changes identity, and a standard
    // scope measures against the standard targets.
    let standard = owner::scoped_context_for_test(
        &fixture.request,
        &Limits::standard(),
        &crate::supervisor::snapshot::attempt::Scope::begin(),
    )
    .unwrap();
    assert_eq!(
        standard.graph_budget().targets(),
        node::GraphTargets::STANDARD
    );
    assert_ne!(standard.identity(), context.identity());
    drop((context, again, standard));
    fixture.retire();
}

/// A Gallery-shaped closure completes cold → repair → warm under the
/// standard accounting targets. The request account, decode and path-check
/// high waters are measured per graph and every target crossing is reported
/// by axis, target and measured figure; none refuses the graph.
#[cfg(unix)]
#[test]
fn gallery_scale_owner_graphs_complete_and_report_targets() {
    let limits = Limits::standard();
    let measured =
        profile(GALLERY_REGISTRY, &limits).unwrap_or_else(|panic| std::panic::resume_unwind(panic));
    assert!(measured.lock >= 80 * 1024, "lock is Gallery-sized");
    let exceeded = exceeded_axes(&measured);
    eprintln!(
        "cohort({GALLERY_REGISTRY}) completed cold → repair → warm; exceedance report: {}",
        if exceeded.is_empty() {
            "no accounting target crossed".to_owned()
        } else {
            exceeded.join("; ")
        }
    );
    // The Gallery-shaped cohort completes cold, repair and warm whatever the
    // accounting targets say. Where a graph crosses the path target the
    // crossing is recorded truthfully; doing less work than the target is the
    // goal, never a failure. The warm graph replays its owner once and every
    // later consumer reuses that replay while its stamps hold.
    let targets = limits.graph_targets();
    assert!(
        measured.warm_graph.paths <= targets.paths,
        "warm: {} path checks; a repeated owner replay returned",
        measured.warm_graph.paths
    );
    for (graph, figures) in [
        ("cold", &measured.cold_graph),
        ("repair", &measured.repair_graph),
    ] {
        let Some(paths) = figures.exceeded.axis(node::PATHS_RESOURCE) else {
            assert!(
                figures.paths <= targets.paths,
                "{graph}: unrecorded crossing"
            );
            continue;
        };
        assert_eq!(paths.target, targets.paths as u64, "{graph}");
        assert_eq!(
            paths.high_water, figures.paths as u64,
            "{graph}: truthful path-check high water"
        );
        assert!(paths.first_requested > paths.target, "{graph}");
        assert!(
            paths.site.is_some(),
            "{graph}: the first exceeding path check is attributed"
        );
    }
    for (graph, figures) in [
        ("cold", &measured.cold_graph),
        ("repair", &measured.repair_graph),
        ("warm", &measured.warm_graph),
    ] {
        for axis in figures.exceeded.axes() {
            assert!(axis.high_water > axis.target, "{graph} {}", axis.name);
            assert!(
                axis.name == node::PATHS_RESOURCE
                    || axis.name == node::DECODED_RESOURCE
                    || axis.name == node::HASHED_RESOURCE
                    || axis.name == node::LOOKUPS_RESOURCE
                    || axis.name == node::DEEP_RESOURCE
                    || axis.name == node::COMPARISONS_RESOURCE,
                "{graph} names a graph axis: {}",
                axis.name
            );
        }
        let crossed_request = figures.request > limits.manifest_bytes;
        let crossed_paths = figures.paths > limits.graph_targets().paths;
        let crossed_decode = figures.decoded > limits.graph_targets().decoded;
        assert_eq!(
            crossed_paths,
            figures.exceeded.axis(node::PATHS_RESOURCE).is_some(),
            "{graph}: the path-check report matches the measured high water"
        );
        assert_eq!(
            crossed_decode,
            figures.exceeded.axis(node::DECODED_RESOURCE).is_some(),
            "{graph}: the decode report matches the measured high water"
        );
        if crossed_request {
            eprintln!(
                "  {graph}: request account crossed its target ({} bytes)",
                figures.request
            );
        }
    }
}

/// Reduced cohorts, profiling per-replay decode and request growth so the
/// Gallery-scale figures can be extrapolated. Every cohort completes
/// and reports its high waters and the axes that crossed their targets.
#[cfg(unix)]
#[test]
fn scaled_owner_graphs_profile_per_replay_accounts() {
    let limits = Limits::standard();
    let mut failed = None;
    for registry in [40, 120] {
        match profile(registry, &limits) {
            Ok(measured) => {
                let exceeded = exceeded_axes(&measured);
                eprintln!(
                    "cohort({registry}) completed cold → repair → warm; exceedance report: {}",
                    if exceeded.is_empty() {
                        "no accounting target crossed".to_owned()
                    } else {
                        exceeded.join("; ")
                    }
                );
            }
            Err(panic) => {
                failed.get_or_insert(panic);
            }
        }
    }
    if let Some(panic) = failed {
        std::panic::resume_unwind(panic);
    }
}

/// Graph output is byte-identical with and without an exceedance.
///
/// First, within one fixture, the warm graph is charged exactly to its path and
/// decode targets before it starts, so every one of its charges crosses a
/// target; `protected_owner_rebuild` already asserts that this warm graph
/// converges on the repair graph's exact input, semantic fingerprints, node
/// contents, actions and artifact. Second, the same cohort under a tiny
/// configured graph profile, run over the same (reused) fixture root, crosses
/// its targets in every graph, records them with truthful high waters, still
/// converges cold → repair → warm, and produces the standard cohort's
/// complete semantic output and generated files.
#[cfg(unix)]
#[test]
fn owner_graph_output_is_identical_with_and_without_exceedance() {
    let standard = Limits::standard();
    PRECHARGE_WARM.set(true);
    let precharged = profile(0, &standard);
    PRECHARGE_WARM.set(false);
    let precharged = precharged.unwrap_or_else(|panic| std::panic::resume_unwind(panic));
    assert!(
        !precharged
            .repair_graph
            .exceeded
            .axis(node::PATHS_RESOURCE)
            .is_some()
            || precharged.repair_graph.paths > standard.graph_targets().paths
    );
    for axis in [node::PATHS_RESOURCE, node::DECODED_RESOURCE] {
        let exceeded = precharged
            .warm_graph
            .exceeded
            .axis(axis)
            .unwrap_or_else(|| panic!("the precharged warm graph crosses {axis}"));
        assert!(exceeded.first_requested > exceeded.target);
        assert!(
            exceeded.site.is_some(),
            "{axis}: the first exceeding site is attributed"
        );
    }
    assert_eq!(
        precharged
            .warm_graph
            .exceeded
            .axis(node::PATHS_RESOURCE)
            .unwrap()
            .high_water,
        precharged.warm_graph.paths as u64
    );
    assert_eq!(
        precharged
            .warm_graph
            .exceeded
            .axis(node::DECODED_RESOURCE)
            .unwrap()
            .high_water,
        precharged.warm_graph.decoded
    );

    let mut tiny = standard;
    tiny.configure("graph-path-checks", 8).unwrap();
    tiny.configure("graph-decode-bytes", 1024).unwrap();
    // The configured cohort reuses the standard cohort's exact (removed) root,
    // so both run over byte-identical paths and differ only in the limits
    // profile.
    FIXTURE_ROOT.with(|root| *root.borrow_mut() = Some(precharged.root.clone()));
    let configured = profile(0, &tiny).unwrap_or_else(|panic| std::panic::resume_unwind(panic));
    assert_eq!(configured.root, precharged.root, "one fixture root");
    // Identical semantic output across target settings, compared
    // between the two cohorts directly: every semantic identity (contract,
    // profile rules, checked, demand, generated, native, build request key
    // and generated manifest) and every generated file's exact bytes must
    // agree, so any target-dependent change to the generated source fails.
    assert_eq!(configured.semantic, precharged.semantic);
    assert!(
        configured.files.len() > 4,
        "every generated file, src/main.rs and its adapter modules, is compared"
    );
    assert_eq!(configured.files, precharged.files);
    // Node contents agree except the checked product, whose header carries
    // its demand's retained-usage figures (items at 8..16, bytes at 16..24).
    // Those are accounting measurements that differ between two runs of the
    // same standard profile at the same root as well (a run-dependent
    // retained path), so only they are normalized; the checked fingerprint,
    // payload and canonical demand bytes must match exactly.
    for slot in [0, 1, 2, 4, 5] {
        assert_eq!(
            configured.contents[slot], precharged.contents[slot],
            "node content {slot}"
        );
    }
    let normalized = |content: &[u8]| {
        let mut content = content.to_vec();
        content[8..24].fill(0);
        content
    };
    assert_eq!(&precharged.checked[..8], b"RAWRCHK1");
    assert_eq!(
        normalized(&configured.checked),
        normalized(&precharged.checked),
        "the checked product agrees outside its retained-usage figures"
    );
    for (graph, figures) in [
        ("cold", &configured.cold_graph),
        ("repair", &configured.repair_graph),
        ("warm", &configured.warm_graph),
    ] {
        let paths = figures
            .exceeded
            .axis(node::PATHS_RESOURCE)
            .unwrap_or_else(|| panic!("{graph} crosses the tiny path-check target"));
        assert_eq!(paths.target, 8);
        assert_eq!(
            paths.high_water, figures.paths as u64,
            "{graph}: truthful high water"
        );
        assert!(
            paths.site.is_some(),
            "{graph}: the first exceeding site is attributed"
        );
        let decoded = figures
            .exceeded
            .axis(node::DECODED_RESOURCE)
            .unwrap_or_else(|| panic!("{graph} crosses the tiny decode target"));
        assert_eq!(decoded.target, 1024);
        assert_eq!(
            decoded.high_water, figures.decoded,
            "{graph}: truthful high water"
        );
    }
}

/// Run one cohort and report its request high water, per-graph account
/// figures and graph decode charges, whether or not it completes.
#[cfg(unix)]
fn profile(registry: usize, limits: &Limits) -> std::thread::Result<Measured> {
    let limit = limits.manifest_bytes;
    node::DECODE_SITES.with(|sites| sites.borrow_mut().clear());
    node::PATH_SITES.with(|sites| sites.borrow_mut().clear());
    owner::REPLAY_TRACE.with(|trace| trace.borrow_mut().clear());
    let result = std::panic::catch_unwind(|| protected_owner_rebuild(registry, limits));
    match &result {
        Ok(measured) => {
            eprintln!(
                "cohort({registry}) request account: lock {} bytes; high water cold {} / repair {} / \
                 warm {} of {limit} bytes",
                measured.lock, measured.cold, measured.repair, measured.warm
            );
            measured.cold_graph.report("cold", limits);
            measured.repair_graph.report("repair", limits);
            measured.warm_graph.report("warm", limits);
        }
        Err(_) => eprintln!(
            "cohort({registry}) request account: failed after high water {} of {limit} bytes \
             (lock {} bytes)",
            high_water(),
            LOCK_BYTES.get()
        ),
    }
    report_decode_sites();
    REGISTRY.set(0);
    result
}

#[cfg(unix)]
fn protected_owner_rebuild(registry: usize, limits: &Limits) -> Measured {
    use std::os::unix::fs::PermissionsExt as _;

    REGISTRY.set(registry);
    let Fixture {
        root,
        request,
        context,
        store,
    } = api_continuation_fixture(limits);
    let limits = *limits;
    let universe = request.universe.clone();
    let cached = cargo::CachedToolchain {
        context: context.clone(),
        attestation: context.cached_toolchain().unwrap(),
    };
    reset_high_water();
    let cold = owner::evaluate_scripted(request, cached, &limits, &crate::metrics::Current, script)
        .unwrap();
    let action = cold.node().receipt().action();
    let prepared = node::Chain::validated(context.clone(), cold.node())
        .prepare_read_set()
        .unwrap();
    prepared.pin_live_last_good().unwrap();
    let api = cold.publish(&limits).unwrap();
    let api_content = api.node_chain().unwrap().parent().content();
    let api_semantics = api.semantic_fingerprint();
    let source = api_source(&limits);
    let adapter = api_adapter(&limits);
    let completed = crate::composition::evaluate_retained_semantic(
        api,
        &adapter,
        &crate::classify::Policy::standard(),
        source,
        &limits,
        |contract, _, _| {
            Ok::<_, ()>((
                contract
                    .compiler((), source, &limits)
                    .unwrap()
                    .seal()
                    .unwrap(),
                ApiChecked,
            ))
        },
    )
    .unwrap();
    let cold_suffix = derived_suffix(completed, &adapter, source, &limits, Expect::Cold, action);
    let cold_high = high_water();
    let cold_graph = GraphFigures::capture(cold_high, context.graph_budget());
    // Shutdown releases the OS guards, but leaves the durable prior roots.
    drop(prepared);
    let old_bundle = ATTEMPT.with(|slot| slot.borrow_mut().take().unwrap());
    let seal = old_bundle.join("seal");
    let frozen = fs::metadata(&seal).unwrap().permissions();
    let mut corrupt = fs::read(&seal).unwrap();
    corrupt[0] ^= 1;
    fs::set_permissions(&seal, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&seal, corrupt).unwrap();
    fs::set_permissions(&seal, frozen).unwrap();
    let predecessor = snapshot(&old_bundle);

    let reopen = || {
        let fresh = crate::context::Context::capture(
            context.project(),
            context.environment().clone(),
            context.target(),
            &limits,
        )
        .unwrap();
        cargo::toolchain::Attestation::fixture_replay(&fresh);
        let mut request = Fixture::request(&root, fresh.environment().clone());
        request.universe = universe.clone();
        let cached = cargo::CachedToolchain {
            attestation: fresh.cached_toolchain().unwrap(),
            context: fresh.clone(),
        };
        (fresh, request, cached)
    };
    reset_high_water();
    let (repair_context, request, cached) = reopen();
    let repaired =
        owner::evaluate_scripted(request, cached, &limits, &crate::metrics::Current, script)
            .expect("protected predecessor cannot block the distinct completed action");
    let repair_action = repaired.node().receipt().action();
    let repair_content = repaired.node().content().digest();
    let repair_completion = *repaired.node().value().completed_identity();
    let fresh_bundle = ATTEMPT.with(|slot| slot.borrow_mut().take().unwrap());
    assert_eq!(
        old_bundle.parent(),
        fresh_bundle.parent(),
        "same execution key"
    );
    assert_ne!(old_bundle, fresh_bundle);
    assert_ne!(action, repair_action);
    let repaired_api = repaired.retain_api(&limits).unwrap();
    assert_eq!(
        owned_identity(&repaired_api.product).1,
        api_content,
        "the entire encoded API Product converges across the protected rebuild"
    );
    let recorder = crate::metrics::recorder::Recorder::transform_at(100_000, 1).unwrap();
    let scope = recorder.scope().unwrap();
    let completed = crate::composition::evaluate_api_semantic(
        repaired_api,
        &adapter,
        &crate::classify::Policy::standard(),
        source,
        &limits,
        |_, _, _| -> std::result::Result<(crate::Demand<crate::Sealed, ()>, ApiChecked), ()> {
            panic!("protected rebuild must reuse the checked source")
        },
    )
    .unwrap();
    assert_eq!(
        completed.contract.raw_graph().semantic_fingerprint(),
        api_semantics
    );
    let repaired_api_identity = completed.contract.raw_fingerprint();
    assert!(completed
        .contract
        .node_chain()
        .unwrap()
        .parent()
        .read_set()
        .contains(repair_action));
    assert!(!completed
        .contract
        .node_chain()
        .unwrap()
        .parent()
        .read_set()
        .contains(action));
    drop(scope);
    assert_prefix_hits(recorder);
    // The repair graph spends its discoverable lookups rechecking the corrupt
    // predecessor after each wait. Crossing the lookup target is
    // recorded, not refused, so the runner lookup proceeds and the cold runner
    // is reused; semantic profile/overlay content converges as before.
    let repaired_suffix = derived_suffix(
        completed,
        &adapter,
        source,
        &limits,
        Expect::Repair,
        repair_action,
    );
    let repair_high = high_water();
    let repair_graph = GraphFigures::capture(repair_high, repair_context.graph_budget());
    assert_ne!(cold_suffix.exact_input, repaired_suffix.exact_input);
    assert_eq!(cold_suffix.semantic, repaired_suffix.semantic);
    assert_eq!(cold_suffix.contents, repaired_suffix.contents);
    assert_eq!(cold_suffix.actions, repaired_suffix.actions);
    #[cfg(feature = "test-fixtures")]
    assert_eq!(cold_suffix.artifact.2, repaired_suffix.artifact.2);
    assert_eq!(snapshot(&old_bundle), predecessor);
    // Two resolution bundles, plus the one cold runner the repair graph reuses.
    assert_eq!(
        crate::store::bundle::count_bundles(store.root()).unwrap(),
        if cfg!(feature = "test-fixtures") {
            3
        } else {
            2
        }
    );
    store.fixture_physical_inventory().unwrap();

    reset_high_water();
    let (warm_context, request, cached) = reopen();
    if PRECHARGE_WARM.get() {
        let graph = warm_context.graph_budget();
        graph.exhaust_paths();
        let (decoded, _, _) = graph.census_usage().unwrap();
        graph
            .decode((limits.graph_targets().decoded - decoded) as usize)
            .unwrap();
        assert!(
            !graph.exceedances().is_exceeded(),
            "exact targets are not exceedances"
        );
    }
    let warm =
        owner::evaluate_scripted(request, cached, &limits, &crate::metrics::Current, no_cold)
            .unwrap();
    assert_eq!(warm.node().receipt().action(), repair_action);
    assert_eq!(warm.node().content().digest(), repair_content);
    assert_eq!(warm.node().value().completed_identity(), &repair_completion);
    let warm_api = warm.publish(&limits).unwrap();
    assert_eq!(warm_api.fingerprint(), repaired_api_identity);
    assert_eq!(
        warm_api.node_chain().unwrap().parent().content(),
        api_content
    );
    assert_eq!(warm_api.semantic_fingerprint(), api_semantics);
    let recorder = crate::metrics::recorder::Recorder::transform_at(100_000, 1).unwrap();
    let scope = recorder.scope().unwrap();
    let completed = crate::composition::evaluate_retained_semantic(
        warm_api,
        &adapter,
        &crate::classify::Policy::standard(),
        source,
        &limits,
        |_, _, _| -> std::result::Result<(crate::Demand<crate::Sealed, ()>, ApiChecked), ()> {
            panic!("warm current graph must reuse the checked source")
        },
    )
    .unwrap();
    drop(scope);
    assert_prefix_hits(recorder);
    let warm_suffix = derived_suffix(
        completed,
        &adapter,
        source,
        &limits,
        Expect::Warm,
        repair_action,
    );
    let warm_high = high_water();
    let warm_graph = GraphFigures::capture(warm_high, warm_context.graph_budget());
    assert_eq!(warm_suffix.exact_input, repaired_suffix.exact_input);
    assert_eq!(warm_suffix.semantic, repaired_suffix.semantic);
    assert_eq!(warm_suffix.contents, repaired_suffix.contents);
    assert_eq!(warm_suffix.actions, repaired_suffix.actions);
    #[cfg(feature = "test-fixtures")]
    assert_eq!(warm_suffix.artifact, repaired_suffix.artifact);
    assert_eq!(warm_suffix.files, repaired_suffix.files);
    assert_eq!(cold_suffix.files, repaired_suffix.files);
    assert_eq!(snapshot(&old_bundle), predecessor);
    drop(warm_context);
    drop(repair_context);
    drop(context);
    // This fixture establishes coexistence and reuse, not post-admission cleanup.
    store.retire_fixture().unwrap();
    fs::remove_dir_all(&root).unwrap();
    REGISTRY.set(0);
    Measured {
        lock: LOCK_BYTES.get(),
        cold: cold_high,
        repair: repair_high,
        warm: warm_high,
        cold_graph,
        repair_graph,
        warm_graph,
        semantic: cold_suffix.semantic,
        contents: cold_suffix.contents,
        files: cold_suffix.files,
        checked: cold_suffix.checked,
        root,
    }
}

/// A later consumer whose stamps all hold reuses the replay's result
/// without replaying; a new stamp on a bundle member replays in full, reusing
/// the staged lock and receipt only while the sealed commitments name them,
/// and current control hashing still refuses drift.
#[cfg(unix)]
#[test]
fn staged_owner_replay_reuses_exact_controls_and_refuses_bundle_drift() {
    use std::os::unix::fs::PermissionsExt as _;

    let Fixture {
        root,
        request,
        context,
        store,
    } = api_continuation_fixture(&Limits::standard());
    let limits = Limits::standard();
    let cached = cargo::CachedToolchain {
        context: context.clone(),
        attestation: context.cached_toolchain().unwrap(),
    };
    let cold = owner::evaluate_scripted(request, cached, &limits, &crate::metrics::Current, script)
        .unwrap();
    let hits = || owner::STAGE_HITS.with(std::cell::Cell::get);
    owner::REPLAY_TRACE.with(|trace| trace.borrow_mut().clear());
    let before = hits();
    let prepared = node::Chain::validated(context.clone(), cold.node())
        .prepare_read_set()
        .unwrap();
    assert_eq!(validates(), 0, "an unchanged consumer does not replay");
    assert_eq!(hits(), before);
    drop(prepared);

    let bundle = ATTEMPT.with(|slot| slot.borrow_mut().take().unwrap());
    let lock =
        crate::cargo::resolution::layout::Paths::observe(bundle.clone(), &node::GraphBudget::new())
            .unwrap()
            .final_form()
            .lock()
            .to_path_buf();
    let frozen = fs::metadata(&lock).unwrap().permissions();
    // Same bytes, new status-change time: the next consumer replays in full.
    fs::set_permissions(&lock, frozen.clone()).unwrap();
    let prepared = node::Chain::validated(context.clone(), cold.node())
        .prepare_read_set()
        .unwrap();
    assert_eq!(validates(), 1, "a changed stamp replays once");
    assert_eq!(
        hits() - before,
        3,
        "the replay reuses lock, receipt and the decoded record"
    );
    drop(prepared);
    let mut drifted = fs::read(&lock).unwrap();
    *drifted.last_mut().unwrap() ^= 1;
    fs::set_permissions(&lock, fs::Permissions::from_mode(0o600)).unwrap();
    fs::write(&lock, drifted).unwrap();
    fs::set_permissions(&lock, frozen).unwrap();
    let before = hits();
    let refused = node::Chain::validated(context.clone(), cold.node())
        .prepare_read_set()
        .err()
        .expect("a staged parse never certifies a drifted bundle lock");
    // Current control hashing classifies the changed member as positive bundle
    // corruption before any staged value could be consulted.
    assert!(
        matches!(refused, node::Error::CandidateCorrupt(_)),
        "{refused:?}"
    );
    assert_eq!(hits(), before, "drift is refused before staged reuse");
    drop(context);
    store.retire_fixture().unwrap();
    fs::remove_dir_all(root).unwrap();
}

/// Decode sites a staged record interpretation must not revisit on a hit replay.
const RECORD_DECODE_FILES: [&str; 4] = [
    "importer/src/input/stored/record.rs",
    "importer/src/input/stored.rs",
    "importer/src/input/stored/build/codec.rs",
    "importer/src/input/stored/record/namespace.rs",
];

/// Graph decode bytes one validate charged at the record interpretation sites.
fn record_decode_bytes(validate: usize) -> u64 {
    owner::REPLAY_TRACE.with(|trace| {
        let (_, files) = &trace.borrow()[validate];
        RECORD_DECODE_FILES
            .iter()
            .map(|file| files.get(*file).copied().unwrap_or(0))
            .sum()
    })
}

fn validates() -> usize {
    owner::REPLAY_TRACE.with(|trace| trace.borrow().len())
}

/// A later consumer in one graph reuses the staged record interpretation
/// without decoding any record wire, while a fresh graph decodes afresh because
/// the stage never crosses evaluations.
#[cfg(unix)]
#[test]
fn staged_owner_record_is_reused_within_one_graph_and_never_across_graphs() {
    let Fixture {
        root,
        request,
        context,
        store,
    } = api_continuation_fixture(&Limits::standard());
    let limits = Limits::standard();
    let universe = request.universe.clone();
    let cached = cargo::CachedToolchain {
        context: context.clone(),
        attestation: context.cached_toolchain().unwrap(),
    };
    owner::REPLAY_TRACE.with(|trace| trace.borrow_mut().clear());
    let hits = || owner::STAGE_HITS.with(Cell::get);
    let cold = owner::evaluate_scripted(request, cached, &limits, &crate::metrics::Current, script)
        .unwrap();
    let cold_validates = validates();
    let before = hits();
    let prepared = node::Chain::validated(context.clone(), cold.node())
        .prepare_read_set()
        .unwrap();
    assert_eq!(
        validates(),
        cold_validates,
        "an unchanged consumer does not replay"
    );
    drop(prepared);
    let bundle = ATTEMPT.with(|slot| slot.borrow().clone().unwrap());
    let record =
        crate::cargo::resolution::layout::Paths::observe(bundle, &node::GraphBudget::new())
            .unwrap()
            .record()
            .to_path_buf();
    fs::set_permissions(&record, fs::metadata(&record).unwrap().permissions()).unwrap();
    let prepared = node::Chain::validated(context.clone(), cold.node())
        .prepare_read_set()
        .unwrap();
    assert_eq!(
        hits() - before,
        3,
        "the consumer after a new stamp reuses lock, receipt and the decoded record"
    );
    assert_eq!(
        validates(),
        cold_validates + 1,
        "one changed stamp, one replay"
    );
    assert_eq!(
        record_decode_bytes(cold_validates),
        0,
        "a hit replay decodes no record, stored, build-codec or namespace wire"
    );
    drop(prepared);
    drop(cold);

    // A fresh Context opens a fresh graph: its first replay decodes the record
    // again, because the first graph's stage is not visible to it.
    let fresh = crate::context::Context::capture(
        context.project(),
        context.environment().clone(),
        context.target(),
        &limits,
    )
    .unwrap();
    cargo::toolchain::Attestation::fixture_replay(&fresh);
    let mut request = Fixture::request(&root, fresh.environment().clone());
    request.universe = universe;
    let cached = cargo::CachedToolchain {
        attestation: fresh.cached_toolchain().unwrap(),
        context: fresh.clone(),
    };
    let warm_first = validates();
    let before = hits();
    let warm =
        owner::evaluate_scripted(request, cached, &limits, &crate::metrics::Current, no_cold)
            .unwrap();
    assert!(validates() > warm_first, "the warm graph replays the owner");
    assert!(
        record_decode_bytes(warm_first) > 0,
        "the second graph's first replay decodes the record afresh"
    );
    let warm_replays = validates() - warm_first;
    assert_eq!(
        hits() - before,
        3 * (warm_replays - 1),
        "every later replay of the warm graph reuses its own staged values"
    );
    for validate in warm_first + 1..validates() {
        assert_eq!(record_decode_bytes(validate), 0, "validate {validate}");
    }
    drop(warm);
    drop(fresh);
    drop(context);
    store.retire_fixture().unwrap();
    fs::remove_dir_all(root).unwrap();
}

/// Record drift between replays, whether a same-length change, a
/// truncation or an extension, is refused at the next consumer as positive
/// bundle corruption before any staged interpretation could be consulted.
#[cfg(unix)]
#[test]
fn staged_owner_record_refuses_record_drift_between_replays() {
    use std::os::unix::fs::PermissionsExt as _;

    let drifts: [(&str, fn(&mut Vec<u8>)); 3] = [
        ("same-length change", |bytes| {
            *bytes.last_mut().unwrap() ^= 1
        }),
        ("truncation", |bytes| {
            bytes.pop();
        }),
        ("extension", |bytes| bytes.push(0)),
    ];
    for (name, drift) in drifts {
        let Fixture {
            root,
            request,
            context,
            store,
        } = api_continuation_fixture(&Limits::standard());
        let limits = Limits::standard();
        let cached = cargo::CachedToolchain {
            context: context.clone(),
            attestation: context.cached_toolchain().unwrap(),
        };
        let hits = || owner::STAGE_HITS.with(Cell::get);
        let cold =
            owner::evaluate_scripted(request, cached, &limits, &crate::metrics::Current, script)
                .unwrap();
        let before = hits();
        let prepared = node::Chain::validated(context.clone(), cold.node())
            .prepare_read_set()
            .unwrap();
        assert_eq!(
            hits(),
            before,
            "{name}: an unchanged consumer does not replay"
        );
        drop(prepared);

        let bundle = ATTEMPT.with(|slot| slot.borrow_mut().take().unwrap());
        let record = crate::cargo::resolution::layout::Paths::observe(
            bundle.clone(),
            &node::GraphBudget::new(),
        )
        .unwrap()
        .record()
        .to_path_buf();
        let frozen = fs::metadata(&record).unwrap().permissions();
        let original = fs::read(&record).unwrap();
        let mut drifted = original.clone();
        drift(&mut drifted);
        assert_ne!(drifted, original);
        fs::set_permissions(&record, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&record, &drifted).unwrap();
        fs::set_permissions(&record, frozen).unwrap();
        let before = hits();
        let refused = node::Chain::validated(context.clone(), cold.node())
            .prepare_read_set()
            .err()
            .unwrap_or_else(|| {
                panic!("{name}: a staged interpretation never certifies a drifted record")
            });
        assert!(
            matches!(refused, node::Error::CandidateCorrupt(_)),
            "{name}: {refused:?}"
        );
        assert_eq!(
            hits(),
            before,
            "{name}: drift is refused before staged reuse"
        );
        drop(cold);
        drop(context);
        store.retire_fixture().unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}

struct Suffix {
    exact_input: [u8; 32],
    semantic: [[u8; 32]; 8],
    contents: [node::Digest; 6],
    actions: [node::ActionKey; 6],
    /// Every generated file's exact bytes, by path.
    files: Vec<(Box<str>, Box<[u8]>)>,
    /// The checked product's exact encoded node content.
    checked: Box<[u8]>,
    #[cfg(feature = "test-fixtures")]
    artifact: (node::ActionKey, PathBuf, [u8; 32]),
}

fn derived_suffix(
    completed: crate::composition::Completed<crate::checked::Retained>,
    adapter: &crate::profile::trusted::Adapter<ApiProfile>,
    source: crate::surface::Snapshot,
    limits: &Limits,
    expect: Expect,
    owner: node::ActionKey,
) -> Suffix {
    let recorder = crate::metrics::recorder::Recorder::transform_at(100_000, 1).unwrap();
    let scope = recorder.scope().unwrap();
    let crate::composition::Completed {
        contract,
        profile,
        facade,
        checked,
    } = completed;
    assert_eq!(
        contract.raw_fingerprint(),
        profile.graph_fingerprint(),
        "profile is rebound to the exact current API"
    );
    let generated =
        crate::composition::profile::prepare_retained(adapter, &profile, &checked, limits).unwrap();
    let symbolic = checked
        .replay_demand((), &contract, source, limits)
        .unwrap()
        .with_trusted_runner(false, &facade, &contract)
        .unwrap();
    let contents = [
        contract.node_chain().unwrap().parent().content(),
        profile.node_chain().unwrap().parent().content(),
        facade.node_chain().parent().content(),
        owned_identity(&checked.product).1,
        owned_identity(&generated.demand).1,
        owned_identity(&generated.generated).1,
    ];
    let actions = [
        contract.node_chain().unwrap().parent().action(),
        profile.node_chain().unwrap().parent().action(),
        facade.node_chain().parent().action(),
        owned_identity(&checked.product).0,
        owned_identity(&generated.demand).0,
        owned_identity(&generated.generated).0,
    ];
    let checked_content =
        node::Value::encode(checked.product.value(), usize::MAX).expect("checked encodes");
    let mut semantic = [[0; 32]; 8];
    semantic[0] = *contract.fingerprint().as_bytes();
    semantic[1] = *profile.rules_digest();
    semantic[2] = checked.checked_fingerprint();
    semantic[3] = *generated.demand_digest();
    semantic[4] = *generated.fingerprint();
    drop(scope);
    // Ordinary callbacks can observe exact identity even when the separately
    // audited source path reuses all of its semantic products.
    let observed_raw = *contract.raw_fingerprint().as_bytes();
    let ordinary = crate::checked::evaluate(&contract, &profile, &facade, source, limits, || {
        Ok::<_, ()>((
            contract
                .compiler((), source, limits)
                .unwrap()
                .seal()
                .unwrap(),
            RawChecked(*contract.raw_fingerprint().as_bytes()),
        ))
    })
    .unwrap();
    assert_eq!(ordinary.checked_payload(), &observed_raw);
    let scope = recorder.scope().unwrap();
    let native = contract.into_native_proof();
    let exact_input = *native.fingerprint().as_bytes();
    semantic[5] = *native.native().unwrap().as_bytes();
    let request = symbolic
        .request_retained(
            native,
            generated,
            crate::supervisor::Owner::new().revision().unwrap(),
        )
        .unwrap();
    semantic[6] = *request.build_request_key().as_bytes();
    let manifest = request.generate(limits).unwrap();
    semantic[7] = manifest.generated_manifest().digest();
    let files = manifest
        .files
        .iter()
        .map(|file| (Box::from(file.path()), Box::from(file.bytes())))
        .collect();
    assert!(manifest
        .chain
        .as_ref()
        .unwrap()
        .parent()
        .read_set()
        .contains(owner));
    #[cfg(feature = "test-fixtures")]
    let artifact = {
        let built = crate::runner::fixture_build(manifest).unwrap();
        assert!(built
            .chain
            .as_ref()
            .unwrap()
            .parent()
            .read_set()
            .contains(owner));
        (
            built.chain.as_ref().unwrap().parent().action(),
            built.artifact().executable().to_path_buf(),
            built.artifact().digest(),
        )
    };
    drop(scope);
    if expect != Expect::Cold {
        // Both the repair and the warm graph reuse the cold runner.
        assert_suffix_hits(recorder, true);
    }
    Suffix {
        exact_input,
        semantic,
        contents,
        actions,
        files,
        checked: checked_content,
        #[cfg(feature = "test-fixtures")]
        artifact,
    }
}

fn owned_identity<K: node::Value>(
    owned: &node::staging::Owned<K>,
) -> (node::ActionKey, node::Digest) {
    match owned {
        node::staging::Owned::Current(value) => {
            (value.receipt().action(), value.content().digest())
        }
        node::staging::Owned::Staged(value) => (value.action(), value.content().digest()),
    }
}

fn assert_prefix_hits(recorder: crate::metrics::recorder::Recorder) {
    use crate::metrics::{NodeEvent, NodeKind};
    let evidence = recorder.snapshot().unwrap();
    assert!(evidence.processes.iter().all(|row| row.count == 0));
    for kind in [
        NodeKind::Generic,
        NodeKind::Profile,
        NodeKind::Facade,
        NodeKind::Checked,
    ] {
        assert_eq!(
            evidence
                .nodes
                .iter()
                .find(|row| row.kind == kind && row.event == NodeEvent::ExecutedMiss)
                .unwrap()
                .count,
            0,
            "{kind:?} must reuse content"
        );
        assert!(
            evidence
                .nodes
                .iter()
                .find(|row| row.kind == kind && row.event == NodeEvent::ValidatedHit)
                .unwrap()
                .count
                > 0,
            "{kind:?} must report a hit"
        );
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Expect {
    Cold,
    Repair,
    Warm,
}

// The owner path folds the native build into the resolution bundle; the
// separate cached NativeProduct node is not evaluated here.
fn assert_suffix_hits(recorder: crate::metrics::recorder::Recorder, runner_reused: bool) {
    use crate::metrics::{NodeEvent, NodeKind};
    let evidence = recorder.snapshot().unwrap();
    assert!(evidence.processes.iter().all(|row| row.count == 0));
    assert!(
        evidence
            .nodes
            .iter()
            .filter(|row| row.kind == NodeKind::NativeProduct)
            .all(|row| row.count == 0),
        "the owner path never evaluates the separate cached NativeProduct node"
    );
    for kind in [NodeKind::ProfileProof, NodeKind::Overlay] {
        assert_eq!(
            evidence
                .nodes
                .iter()
                .find(|row| row.kind == kind && row.event == NodeEvent::ExecutedMiss)
                .unwrap()
                .count,
            0,
            "{kind:?} must reuse content"
        );
        assert!(
            evidence
                .nodes
                .iter()
                .find(|row| row.kind == kind && row.event == NodeEvent::ValidatedHit)
                .unwrap()
                .count
                > 0,
            "{kind:?} must report a hit"
        );
    }
    #[cfg(feature = "test-fixtures")]
    {
        let runner = |event| {
            evidence
                .nodes
                .iter()
                .find(|row| row.kind == NodeKind::Runner && row.event == event)
                .unwrap()
                .count
        };
        let (miss, hit, rejected) = if runner_reused { (0, 1, 0) } else { (1, 0, 1) };
        assert_eq!(runner(NodeEvent::ExecutedMiss), miss);
        assert_eq!(runner(NodeEvent::ValidatedHit), hit);
        assert_eq!(runner(NodeEvent::RejectedHit), rejected);
    }
    #[cfg(not(feature = "test-fixtures"))]
    let _ = runner_reused;
}

struct ApiProfile;
impl crate::profile::trusted::Operations for ApiProfile {
    fn classify<P: crate::api::Phase>(
        &self,
        _: &crate::api::Graph<P>,
        _: &mut crate::profile::contract::Builder<'_, P>,
    ) -> std::result::Result<(), crate::profile::Error> {
        Ok(())
    }
    fn demand(
        &self,
        _: crate::profile::trusted::Contract<'_>,
        _: crate::profile::trusted::Checked<'_>,
        _: &mut crate::profile::demand::Builder<'_>,
    ) -> std::result::Result<(), crate::profile::Error> {
        Ok(())
    }
    fn generate(
        &self,
        _: crate::profile::trusted::Demand<'_>,
        _: &mut crate::profile::demand::Generator<'_>,
    ) -> std::result::Result<(), crate::profile::Error> {
        Ok(())
    }
}
struct RawChecked([u8; 32]);
impl crate::profile::Checked for RawChecked {
    fn profile_fingerprint(&self) -> [u8; 32] {
        self.0
    }
    fn profile_payload(&self) -> &[u8] {
        &self.0
    }
}

struct ApiChecked;
impl crate::profile::Checked for ApiChecked {
    fn profile_fingerprint(&self) -> [u8; 32] {
        [9; 32]
    }
    fn profile_payload(&self) -> &[u8] {
        &[9; 32]
    }
}
fn api_adapter(limits: &Limits) -> crate::profile::trusted::Adapter<ApiProfile> {
    crate::profile::trusted::Adapter::trusted(
        ApiProfile,
        crate::profile::Identity::new("api.continuation.fixture", limits).unwrap(),
        crate::profile::Version::new(1).unwrap(),
        false,
    )
    .semantic_api()
}
fn api_source(limits: &Limits) -> crate::surface::Snapshot {
    let mut source = crate::surface::Snapshot::builder(limits);
    source.entry("/fixture/Cargo.toml").unwrap();
    source
        .file("/fixture/main.rs", b"let value = 1;")
        .unwrap();
    source.seal().unwrap()
}

#[test]
fn retained_api_composes_actual_owner_then_reuses_without_children() {
    let Fixture {
        root,
        request,
        context,
        store,
    } = api_continuation_fixture(&Limits::standard());
    let limits = Limits::standard();
    let universe = request.universe.clone();
    let cached = || cargo::CachedToolchain {
        context: context.clone(),
        attestation: context.cached_toolchain().unwrap(),
    };
    let api =
        owner::evaluate_scripted(request, cached(), &limits, &crate::metrics::Current, script)
            .unwrap()
            .retain_api(&limits)
            .unwrap();
    assert!(
        matches!(&api.product, node::staging::Owned::Staged(_)),
        "cold API does not promote before classification"
    );
    let source = api_source(&limits);
    let adapter = api_adapter(&limits);
    let completed = crate::composition::evaluate_api_semantic(
        api,
        &adapter,
        &crate::classify::Policy::standard(),
        source,
        &limits,
        |contract, _, _| {
            Ok::<_, ()>((
                contract
                    .compiler((), source, &limits)
                    .unwrap()
                    .seal()
                    .unwrap(),
                ApiChecked,
            ))
        },
    )
    .unwrap();
    let mut request = Fixture::request(&root, context.environment().clone());
    request.universe = universe;
    test_launch::Script::new(
        root.clone(),
        test_launch::FileLimits { count: 0, bytes: 0 },
        Vec::new(),
    )
    .unwrap()
    .run(|| {
        let warm = owner::evaluate_scripted(
            request,
            cached(),
            &limits,
            &crate::metrics::Current,
            no_cold,
        )
        .unwrap()
        .retain_api(&limits)
        .unwrap();
        assert!(matches!(&warm.product, node::staging::Owned::Current(_)));
        let warm = crate::composition::evaluate_api_semantic(
            warm,
            &adapter,
            &crate::classify::Policy::standard(),
            source,
            &limits,
            |_, _, _| -> std::result::Result<(crate::Demand<crate::Sealed, ()>, ApiChecked), ()> {
                panic!("warm source must not run")
            },
        )
        .unwrap();
        assert_eq!(
            warm.contract.fingerprint(),
            completed.contract.fingerprint()
        );
        assert_eq!(
            warm.checked.checked_payload(),
            completed.checked.checked_payload()
        );
    })
    .unwrap();
    drop(completed);
    drop(context);
    store.retire_fixture().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn resolved_warm_launch_equals_cold_without_the_api_graph() {
    let Fixture {
        root,
        request,
        context,
        store,
    } = api_continuation_fixture(&Limits::standard());
    let limits = Limits::standard();
    let universe = request.universe.clone();
    let cached = || cargo::CachedToolchain {
        context: context.clone(),
        attestation: context.cached_toolchain().unwrap(),
    };
    let policy = crate::classify::Policy::standard();
    let adapter = api_adapter(&limits);
    let source = api_source(&limits);
    // This owner exposes no demandable item; plan round trips are covered
    // by the composition fixture (`resolved_plans_replay_like_the_contract`).
    let seal = |contract: &crate::Contract<crate::Classified>, source| {
        contract
            .compiler((), source, &limits)
            .unwrap()
            .seal()
            .unwrap()
    };
    let retain = |request, script| {
        owner::evaluate_scripted(request, cached(), &limits, &crate::metrics::Current, script)
            .unwrap()
            .retain_api(&limits)
            .unwrap()
    };
    let resolve = |api, source| {
        crate::composition::resolve_api_semantic(api, &adapter, &policy, source, &limits, |_| {
            Ok::<_, ()>(())
        })
        .unwrap()
    };
    // Cold: no resolved product yet; the full path runs and records it.
    let Err(api) = resolve(retain(request, script), source) else {
        panic!("a cold launch has no resolved demand");
    };
    let crate::composition::Completed {
        contract,
        profile,
        facade,
        checked,
    } = crate::composition::evaluate_api_semantic(
        api,
        &adapter,
        &policy,
        source,
        &limits,
        |contract, _, _| Ok::<_, ()>((seal(contract, source), ApiChecked)),
    )
    .unwrap();
    let cold = checked
        .replay_demand((), &contract, source, &limits)
        .unwrap()
        .with_trusted_runner(true, &facade, &contract)
        .unwrap();
    crate::composition::record_resolved(&contract, &cold, &adapter, &policy).unwrap();
    let plans = serde_json::to_vec(cold.adapter_plans()).unwrap();
    let native = contract.into_native_proof();
    let request = || {
        let mut request = Fixture::request(&root, context.environment().clone());
        request.universe = universe.clone();
        request
    };
    test_launch::Script::new(
        root.clone(),
        test_launch::FileLimits { count: 0, bytes: 0 },
        Vec::new(),
    )
    .unwrap()
    .run(|| {
        // Warm: unchanged inputs hit, and the deferred graph rebuild never runs.
        let mut api = retain(request(), no_cold);
        assert!(matches!(api.graph, crate::api::retained::Lazy::Pending(_)));
        api.graph = crate::api::retained::Lazy::Pending(Box::new(|_| {
            panic!("a resolved warm launch must not rebuild the API graph")
        }));
        let started = std::time::Instant::now();
        let Ok(warm) = resolve(api, source) else {
            panic!("unchanged inputs reuse the resolved demand");
        };
        eprintln!("resolved warm hit: {:?}", started.elapsed());
        assert_eq!(warm.demand.canonical_bytes(), cold.canonical_bytes());
        assert_eq!(warm.demand.fingerprint(), cold.fingerprint());
        assert_eq!(
            warm.demand.contract_fingerprint(),
            cold.contract_fingerprint()
        );
        assert_eq!(
            warm.demand.cached_retained_usage(),
            cold.cached_retained_usage()
        );
        assert_eq!(
            serde_json::to_vec(warm.demand.adapter_plans()).unwrap(),
            plans
        );
        assert_eq!(warm.profile.rules_digest(), profile.rules_digest());
        assert_eq!(warm.facade.bytes(), facade.bytes());
        assert_eq!(warm.checked.checked_payload(), checked.checked_payload());
        assert_eq!(warm.native.fingerprint(), native.fingerprint());
        assert_eq!(
            warm.native.native().unwrap().as_bytes(),
            native.native().unwrap().as_bytes()
        );
        assert!(warm.native.watch().eq(native.watch()));
        let (warm, cold) = (warm.demand.into_request(), cold.into_request());
        assert_eq!(
            crate::bridge::generate::semantic_digest(&warm).unwrap(),
            crate::bridge::generate::semantic_digest(&cold).unwrap()
        );
        // A changed Rawr source (a new demand) misses and keeps the full path.
        let mut edit = crate::surface::Snapshot::builder(&limits);
        edit.entry("/fixture/Cargo.toml").unwrap();
        edit.file("/fixture/main.rs", b"let value = 2;").unwrap();
        assert!(resolve(retain(request(), no_cold), edit.seal().unwrap()).is_err());
    })
    .unwrap();
    drop((profile, facade, checked, native));
    drop(context);
    store.retire_fixture().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn retained_api_rejects_owner_drift_before_source_callback() {
    let Fixture {
        root,
        request,
        context,
        store,
    } = api_continuation_fixture(&Limits::standard());
    let limits = Limits::standard();
    let cached = cargo::CachedToolchain {
        context: context.clone(),
        attestation: context.cached_toolchain().unwrap(),
    };
    let api = owner::evaluate_scripted(request, cached, &limits, &crate::metrics::Current, script)
        .unwrap()
        .retain_api(&limits)
        .unwrap();
    fs::write(
        root.join("authority/.cargo/config.toml"),
        b"[net]\noffline = false\n",
    )
    .unwrap();
    assert!(crate::composition::evaluate_api(
        api,
        &api_adapter(&limits),
        &crate::classify::Policy::standard(),
        api_source(&limits),
        &limits,
        |_, _, _| -> std::result::Result<(crate::Demand<crate::Sealed, ()>, ApiChecked), ()> {
            panic!("owner drift must reject before whole source callback")
        }
    )
    .is_err());
    drop(context);
    store.retire_fixture().unwrap();
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn retained_api_rejects_foreign_composition_context() {
    let Fixture {
        root,
        request,
        context,
        store,
    } = api_continuation_fixture(&Limits::standard());
    let other = Fixture::open();
    let limits = Limits::standard();
    let cached = cargo::CachedToolchain {
        context: context.clone(),
        attestation: context.cached_toolchain().unwrap(),
    };
    let mut api =
        owner::evaluate_scripted(request, cached, &limits, &crate::metrics::Current, script)
            .unwrap()
            .retain_api(&limits)
            .unwrap();
    api.publication.context = other.context.clone();
    assert!(crate::composition::evaluate_api(
        api,
        &api_adapter(&limits),
        &crate::classify::Policy::standard(),
        api_source(&limits),
        &limits,
        |_, _, _| -> std::result::Result<(crate::Demand<crate::Sealed, ()>, ApiChecked), ()> {
            panic!("foreign account must reject before callback")
        }
    )
    .is_err());
    drop(context);
    store.retire_fixture().unwrap();
    fs::remove_dir_all(root).unwrap();
    drop(other.context);
    other.store.retire_fixture().unwrap();
    fs::remove_dir_all(other.root).unwrap();
}

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

use crate::profile::{self, Definition as _};

static NEXT: AtomicU64 = AtomicU64::new(0);

pub(super) struct Root(pub(super) PathBuf);
impl Root {
    pub(super) fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "rawr-composition-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("fixture directory");
        fs::create_dir(path.join("cargo-home")).expect("fixture Cargo home");
        Self(fs::canonicalize(path).expect("canonical fixture directory"))
    }
}
impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Api;
impl crate::node::Value for Api {
    const KIND: crate::node::Kind = crate::node::Kind::Api;
    const SCHEMA: u32 = u32::MAX;
    fn encode(&self, _: usize) -> Result<Box<[u8]>, crate::node::Error> {
        Ok(Box::from(&b"composition-api"[..]))
    }
    fn decode(bytes: &[u8], _: usize) -> Result<Self, crate::node::Error> {
        if bytes == b"composition-api" {
            Ok(Self)
        } else {
            Err(crate::node::Error::Codec("fixture API changed"))
        }
    }
}

pub(super) struct Checked;
impl profile::Checked for Checked {
    fn profile_fingerprint(&self) -> [u8; 32] {
        [0x63; 32]
    }
    fn profile_payload(&self) -> &[u8] {
        b"checked-payload"
    }
}

struct Definition(Arc<AtomicUsize>);
impl profile::Definition<Checked> for Definition {
    fn identity(&self) -> profile::Identity {
        profile::Identity::new(
            "rawr.composition.fixture",
            &crate::cargo::Limits::standard(),
        )
        .expect("profile identity")
    }
    fn version(&self) -> profile::Version {
        profile::Version::new(1).expect("profile version")
    }
    fn classify(
        &self,
        _: &crate::api::Graph<crate::api::Extracted>,
        _: &mut profile::contract::Builder<'_>,
    ) -> Result<(), profile::Error> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    fn demand(
        &self,
        _: &profile::Contract,
        _: &Checked,
        _: &mut profile::demand::Builder<'_>,
    ) -> Result<(), profile::Error> {
        Ok(())
    }
    fn generate(
        &self,
        _: &profile::Demand,
        _: &mut profile::demand::Generator<'_>,
    ) -> Result<(), profile::Error> {
        Ok(())
    }
}
impl profile::trusted::Operations for &Definition {
    fn classify<P: crate::api::Phase>(
        &self,
        graph: &crate::api::Graph<P>,
        output: &mut profile::contract::Builder<'_, P>,
    ) -> Result<(), profile::Error> {
        let _ = (graph, output);
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }
    fn demand(
        &self,
        _: profile::trusted::Contract<'_>,
        _: profile::trusted::Checked<'_>,
        _: &mut profile::demand::Builder<'_>,
    ) -> Result<(), profile::Error> {
        Ok(())
    }
    fn generate(
        &self,
        _: profile::trusted::Demand<'_>,
        _: &mut profile::demand::Generator<'_>,
    ) -> Result<(), profile::Error> {
        Ok(())
    }
}

pub(super) fn graph(
    root: &Path,
    calls: &Arc<AtomicUsize>,
    limits: &crate::cargo::Limits,
) -> crate::api::Graph<crate::api::Extracted> {
    graph_with_change(
        root,
        calls,
        Arc::new(std::sync::atomic::AtomicBool::new(false)),
        limits,
    )
}

pub(super) fn graph_with_change(
    root: &Path,
    calls: &Arc<AtomicUsize>,
    changed: Arc<std::sync::atomic::AtomicBool>,
    limits: &crate::cargo::Limits,
) -> crate::api::Graph<crate::api::Extracted> {
    let cargo_home = root.join("cargo-home");
    let context = crate::context::Context::capture(
        root,
        crate::cargo::config::Environment::from_pairs([(
            "CARGO_HOME",
            cargo_home.to_str().expect("Unicode Cargo home"),
        )])
        .expect("environment"),
        "aarch64-apple-darwin",
        limits,
    )
    .expect("context");
    let store = context.open_store().expect("store");
    let recipe = context
        .discover::<Api>(b"composition-api", limits.node_output_bytes())
        .expect("API recipe");
    let calls = Arc::clone(calls);
    let api = crate::node::Evaluator::new(&store, &crate::metrics::Current)
        .discover(
            &recipe,
            move |_| {
                calls.fetch_add(1, Ordering::Relaxed);
                if changed.load(Ordering::Relaxed) {
                    Ok(Box::from(&b"changed-api-input"[..]))
                } else {
                    Ok(Box::from(&b"owned-api-input"[..]))
                }
            },
            || Ok(Api),
        )
        .expect("API discovery");
    let retained =
        crate::node::read_set::Retention::from_validated(&api, None).expect("API retention");
    let chain = crate::node::Chain::validated_retained(context, &api, retained)
        .expect("retained API chain");
    crate::test_fixture::graph().with_node_chain_for_test(chain)
}

pub(super) fn snapshot(limits: &crate::cargo::Limits) -> crate::surface::Snapshot {
    let mut builder = crate::surface::Snapshot::builder(limits);
    builder.entry("/fixture/rawr.toml").expect("entry");
    builder.file("/fixture/main.rawr", b"Pair").expect("source");
    builder.seal().expect("snapshot")
}

pub(super) fn demand(
    contract: &crate::Contract<crate::Classified>,
    source: crate::surface::Snapshot,
    limits: &crate::cargo::Limits,
) -> crate::Demand<crate::Sealed, ()> {
    let identity = contract
        .outcomes()
        .find(|outcome| outcome.representability().is_demandable())
        .expect("demandable outcome")
        .identity()
        .clone();
    let mut compiler = contract.compiler((), source, limits).expect("compiler");
    compiler
        .require(
            identity,
            crate::surface::Origin::checked("/fixture/main.rawr", 0, 4, limits).expect("origin"),
        )
        .expect("require");
    compiler.seal().expect("demand")
}

#[test]
fn connected_prefix_reuses_warm_receipts_without_mutable_union_or_source_work() {
    // Every dependency-consistent seeded prefix, including cold and all-hit.
    // Facade-present/generic-absent requires selective eviction and is separate.
    for (has_profile, has_generic, has_facade) in [
        (false, false, false),
        (true, false, false),
        (false, true, false),
        (true, true, false),
        (false, true, true),
        (true, true, true),
    ] {
        let root = Root::new();
        let limits = crate::cargo::Limits::standard();
        let witnesses = Arc::new(AtomicUsize::new(0));
        let profile_calls = Arc::new(AtomicUsize::new(0));
        let definition = Definition(Arc::clone(&profile_calls));
        let policy = crate::classify::Policy::standard();
        let seed = graph(&root.0, &witnesses, &limits);
        if has_profile {
            profile::classify(&definition, &seed, &limits).expect("seed profile");
        }
        if has_generic {
            let contract = seed.classify(&policy, &limits).expect("seed generic");
            if has_facade {
                crate::surface::facade::Plan::encode_cached(&contract, &limits)
                    .expect("seed facade");
            }
        } else {
            drop(seed);
        }
        let adapter = profile::trusted::Adapter::trusted(
            &definition,
            definition.identity(),
            definition.version(),
            false,
        );
        let source = snapshot(&limits);
        let source_calls = AtomicUsize::new(0);
        let cold = super::evaluate(
            graph(&root.0, &witnesses, &limits),
            &adapter,
            &policy,
            source,
            &limits,
            |contract, _, _| {
                source_calls.fetch_add(1, Ordering::Relaxed);
                Ok::<_, ()>((demand(contract, source, &limits), Checked))
            },
        )
        .expect("connected initial prefix");
        assert_eq!(
            profile_calls.load(Ordering::Relaxed),
            1,
            "profile computed once across seed and composition"
        );
        assert_eq!(source_calls.load(Ordering::Relaxed), 1);
        let fresh = graph(&root.0, &witnesses, &limits);
        witnesses.store(0, Ordering::Relaxed);
        let warm = super::evaluate(
            fresh,
            &adapter,
            &policy,
            source,
            &limits,
            |contract, _, _| {
                source_calls.fetch_add(1, Ordering::Relaxed);
                Ok::<_, ()>((demand(contract, source, &limits), Checked))
            },
        )
        .expect("connected fresh warm prefix");
        assert_eq!(
            witnesses.load(Ordering::Relaxed),
            0,
            "warm prefix adds no mutable ancestor union"
        );
        assert_eq!(profile_calls.load(Ordering::Relaxed), 1);
        assert_eq!(
            source_calls.load(Ordering::Relaxed),
            1,
            "warm checked hit does not compile source"
        );
        assert_eq!(cold.contract.fingerprint(), warm.contract.fingerprint());
        assert_eq!(cold.profile.rules_digest(), warm.profile.rules_digest());
        assert_eq!(
            cold.checked.checked_fingerprint(),
            warm.checked.checked_fingerprint()
        );
        assert_eq!(
            cold.checked.checked_payload(),
            warm.checked.checked_payload()
        );
        warm.checked
            .replay_demand((), &warm.contract, source, &limits)
            .expect("warm symbolic demand replays");
        let mut edit = crate::surface::Snapshot::builder(&limits);
        edit.entry("/fixture/rawr.toml").expect("edited entry");
        edit.file("/fixture/main.rawr", b"Pair // edited")
            .expect("edited source");
        let edited_source = edit.seal().expect("edited snapshot");
        let fresh = graph(&root.0, &witnesses, &limits);
        witnesses.store(0, Ordering::Relaxed);
        let edited = super::evaluate(
            fresh,
            &adapter,
            &policy,
            edited_source,
            &limits,
            |contract, _, _| {
                source_calls.fetch_add(1, Ordering::Relaxed);
                Ok::<_, ()>((demand(contract, edited_source, &limits), Checked))
            },
        )
        .expect("Rawr-only edit prefix");
        assert_eq!(
            profile_calls.load(Ordering::Relaxed),
            1,
            "Rawr edit retains profile classification"
        );
        assert_eq!(
            source_calls.load(Ordering::Relaxed),
            2,
            "Rawr edit runs checked source once"
        );
        assert!(
            witnesses.load(Ordering::Relaxed) > 0,
            "actual source miss preserves mutable parent validation"
        );
        assert_eq!(edited.contract.fingerprint(), warm.contract.fingerprint());
        assert_eq!(edited.profile.rules_digest(), warm.profile.rules_digest());
        edited
            .checked
            .replay_demand((), &edited.contract, edited_source, &limits)
            .expect("edited symbolic demand replays");
    }
}

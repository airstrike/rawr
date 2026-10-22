//! Complete synthetic public Cargo and Probe observations under one real owner.

use std::fs;
use std::path::Path;

use serde_json::{json, Value};

use super::super::{expected_reply, Command, Fixture};
use super::built::{artifacts, fresh_steps};
use super::{
    bind_universe, bind_universe_seeded, documents, documents_scaled, render_lock, seed_lock,
    LockScope,
};
use crate::cargo::{self, resolution::layout::Layout, unit, Limits};
use crate::extract::protocol::attempt;
use crate::process::{self, test_launch, ToolPaths as _};
use crate::{Error, Result};

mod owner;

#[derive(Clone, Copy)]
enum Outcome {
    Success,
    ProbeFailure,
    CorruptSidecar,
    ConfigExceedance,
}

fn probe(
    layout: &Layout,
    native: &process::Native,
    context: &crate::context::Context,
    outcome: Outcome,
) -> test_launch::Step {
    let native = native.production();
    let executable = native.driver().to_path_buf();
    let arguments = vec![
        "--probe".into(),
        native.rustc().as_os_str().to_owned(),
        layout.extraction().plan().as_os_str().to_owned(),
        layout.extraction().protocol().as_os_str().to_owned(),
        layout.extraction().source().as_os_str().to_owned(),
        context.rawr().join("extraction").into_os_string(),
    ];
    let directory = layout.extraction().root().to_path_buf();
    let plan_path = layout.extraction().plan().to_path_buf();
    let protocol = layout.extraction().protocol().to_path_buf();
    let root = layout.root().to_path_buf();
    test_launch::Step::new(
        move |_| {
            test_launch::Expected::new(
                executable,
                process::Command::probe(arguments, directory, Box::default())
                    .expect("exact Probe command seals"),
                Limits::standard().process_limits(),
            )
        },
        move |_| {
            if matches!(outcome, Outcome::ProbeFailure) {
                return test_launch::Reply::exit(
                    1,
                    Vec::new(),
                    b"fixture Probe failure".to_vec(),
                    Vec::new(),
                );
            }
            let plan: Value = serde_json::from_slice(
                &fs::read(plan_path).expect("the actual Probe plan published before launch"),
            )
            .expect("plan parses");
            let entries = plan["entries"].as_array().expect("plan entries exist");
            let arguments_digest = "2".repeat(64);
            let mut files = Vec::new();
            for (index, entry) in entries.iter().enumerate() {
                let id = format!("{:016x}", index + 1);
                let artifact = &entry["artifact"];
                let fragment = json!({"handshake": {
                "schema": plan["protocol_schema"], "package": entry["package"],
                "root": entry["root"], "target": entry["target"],
                "target_kind": entry["target_kind"], "target_kinds": entry["target_kinds"],
                "crate_types": entry["crate_types"], "crate_name": entry["crate_name"],
                "edition": entry["edition"], "profile": entry["profile"],
                "compilation": "target", "host": plan["host"],
                "selected_target": plan["selected_target"], "features": entry["features"],
                "probe_arguments_digest": arguments_digest, "compiler_commit": plan["compiler_commit"],
                "invocation_nonce": entry["invocation_nonce"], "unit_correlation": entry["unit_correlation"],
                "unit_key": entry["unit_key"], "dependencies": entry["dependencies"],
                "artifact": {"path": artifact["path"], "length": artifact["length"],
                    "digest": artifact["digest"], "profile": artifact["profile"], "fresh": artifact["fresh"],
                    "crate_hash": "fixture-crate-hash", "stable_crate_id": id,
                    "probe_alias": entry["root"]["aliases"][0]},
                "sysroot": [], "loaded_crates": [{"crate_name": entry["crate_name"],
                    "stable_crate_id": id, "artifacts": [artifact["path"]]}], "external_crates": []},
                "reachable": [], "definitions": [], "implementations": [], "scalar_predicate_proofs": []});
                let bytes = if matches!(outcome, Outcome::CorruptSidecar) && index == 0 {
                    b"{}".to_vec()
                } else {
                    serde_json::to_vec(&fragment).expect("synthetic public fragment encodes")
                };
                let correlation = entry["unit_correlation"]
                    .as_str()
                    .expect("correlation exists");
                let path = protocol.join(format!("{correlation}.json"));
                files.push(test_launch::File::new(
                    path.strip_prefix(&root)
                        .expect("sidecar belongs to this Attempt")
                        .to_path_buf(),
                    bytes,
                ));
            }
            let invocations = entries
                .iter()
                .map(|entry| attempt::Invocation {
                    nonce: entry["invocation_nonce"]
                        .as_str()
                        .expect("invocation nonce exists"),
                    correlation: entry["unit_correlation"]
                        .as_str()
                        .expect("correlation exists"),
                })
                .collect::<Vec<_>>();
            let mut completion = Vec::new();
            attempt::encode(
                &mut completion,
                &arguments_digest,
                true,
                &invocations,
                [].into_iter(),
                std::iter::empty(),
            )
            .expect("synthetic conservative attempt completion encodes");
            files.push(test_launch::File::new(
                protocol
                    .join("attempts")
                    .strip_prefix(&root)
                    .expect("attempt completion belongs to this Attempt")
                    .to_path_buf(),
                completion,
            ));
            test_launch::Reply::exit(0, Vec::new(), Vec::new(), files)
        },
    )
}

fn exercise(outcome: Outcome) {
    let Fixture {
        root,
        mut request,
        context,
        store,
    } = Fixture::open();
    Fixture::reserve_owned_parts(&context, &store, |owned, native| {
        let layout = &owned;
        let limits = Limits::standard();
        let mut cargo_limits = limits;
        cargo_limits.items -= 1;
        let cached = cargo::CachedToolchain {
            context: context.clone(),
            attestation: context
                .cached_toolchain()
                .expect("actual fixture attestation"),
        };
        let (metadata, units) = documents(&request, layout.final_form());
        bind_universe(&mut request, &cached);
        let mut steps = fresh_steps(&request, native, layout, &metadata, &units);
        let (build_bytes, files) = artifacts(layout, &metadata, &request.target);
        steps.push(expected_reply(
            &request,
            native,
            layout.final_form(),
            Command::Build,
            true,
            move |_| test_launch::Reply::exit(0, build_bytes, Vec::new(), files),
        ));
        steps.push(probe(layout, native, &context, outcome));
        let script = test_launch::Script::new(
            layout.root().to_path_buf(),
            test_launch::FileLimits {
                count: 16,
                bytes: 128 * 1024,
            },
            steps,
        )
        .expect("bounded complete script seals");
        let mut allocation = request.allocation;
        script
            .run_result(|| -> Result<()> {
                let mut owned = Some(owned);
                let layout = owned.as_ref().expect("reserved layout");
                let harness = request.prepare_fresh_harness_in(
                    layout,
                    &cached,
                    &limits,
                    &cargo_limits,
                    &mut allocation,
                    unit::Allocation::begin(&cargo_limits),
                )?;
                let built = request.build_harness_in(
                    &cached,
                    layout,
                    harness,
                    &mut allocation,
                    &limits,
                    &cargo_limits,
                )?;
                let before = built.allocation.bytes;
                let documents = built
                    .harness
                    .documents
                    .as_ref()
                    .expect("the real cold entry retained both documents");
                let metadata_pointer = documents.metadata.bytes().as_ptr();
                let unit_pointer = documents.units.bytes().as_ptr();
                built
                    .harness
                    .metadata
                    .require_exact_raw(documents.metadata.bytes(), "retained fixture")?;
                built
                    .harness
                    .initial
                    .require_exact_raw(documents.units.bytes(), "retained fixture")?;
                let build_storage = built.build.fixture_storage();
                let build_usage = built.build.usage();
                if matches!(outcome, Outcome::ConfigExceedance) {
                    let paths = layout.final_form();
                    let copies = [paths.manifest(), paths.source(), paths.lock()]
                        .iter()
                        .map(|path| path.as_os_str().len())
                        .sum::<usize>();
                    // Leave one byte less than the three retained path
                    // copies. The account is a target: completion proceeds and
                    // the last copy is recorded crossing by exactly that byte.
                    built
                        .allocation
                        .bytes(built.allocation.byte_limit - built.allocation.bytes - copies + 1)?;
                    assert!(!built.allocation.exceedances().is_exceeded());
                }
                let replay_definition = request
                    .universe
                    .as_ref()
                    .expect("current catalog")
                    .current()
                    .clone();
                let result =
                    request.complete_harness_in(&cached, layout, built, &limits, &cargo_limits);
                assert!(Path::new(layout.extraction().plan()).is_file());
                match outcome {
                    Outcome::Success => {
                        let mut resolution = result?;
                        assert_eq!(resolution.harness_root(), layout.final_form().root());
                        assert_eq!(resolution.toolchain(), cached.attestation.value().proof());
                        assert!(resolution.dag.is_none());
                        assert!(resolution.universe.is_some());
                        resolution.cargo_allocation.require(&cargo_limits)?;
                        assert!(allocation.bytes > before);
                        let receipt: Value = serde_json::from_slice(
                            &fs::read(layout.receipt())
                                .expect("only complete observations publish the assigned receipt"),
                        )
                        .expect("receipt is valid JSON");
                        assert!(receipt.is_object());
                        assert!(!layout.root().join(cargo::UNIVERSE_RECEIPT).exists());
                        assert!(resolution.input.universe_build().is_some());
                        let retained = resolution
                            .observations
                            .as_ref()
                            .expect("completion retained one paired observation");
                        assert_eq!(
                            retained.documents.metadata.bytes().as_ptr(),
                            metadata_pointer
                        );
                        assert_eq!(retained.documents.units.bytes().as_ptr(), unit_pointer);
                        assert_eq!(retained.build.build().fixture_storage(), build_storage);
                        assert_eq!(retained.build.build().usage(), build_usage);
                        resolution.metadata.require_exact_raw(
                            retained.documents.metadata.bytes(),
                            "complete retained fixture",
                        )?;
                        resolution.build_unit.require_exact_raw(
                            retained.documents.units.bytes(),
                            "complete retained fixture",
                        )?;
                        let (stored, mut original) = crate::input::stored::Stored::capture(
                            resolution.input,
                            &limits,
                            store.graph_budget(),
                        )
                        .into_parts();
                        let stored =
                            stored.expect("the complete cold fixture retains its full input data");
                        let mut budget = crate::input::stored::Budget::borrow(
                            &limits,
                            &mut original,
                            store.graph_budget(),
                        );
                        let encoded = crate::input::stored::Record::encode(
                            retained.documents.metadata.bytes(),
                            retained.documents.units.bytes(),
                            &retained.build,
                            &stored,
                            &mut budget,
                        )
                        .expect("one complete historical record encodes");
                        let expected_input_schema = stored.schema();
                        let expected_input_fingerprint = stored.fingerprint();
                        let expected_input_entries = stored.entries().to_vec();
                        drop(stored);
                        let record = crate::input::stored::Record::decode(&encoded, &mut budget)
                            .expect("complete historical record frames");
                        let saved = record
                            .persist(layout, &mut budget)
                            .expect("historical record persists inside the reserved bundle");
                        assert!(record.persist(layout, &mut budget).is_err());
                        let modified = fs::metadata(layout.paths().record())
                            .expect("persisted metadata")
                            .modified()
                            .expect("persisted mtime");
                        let persisted = saved
                            .read(layout.paths(), &mut budget)
                            .expect("persisted historical record reads without repair");
                        assert_eq!(persisted, encoded);
                        assert_eq!(
                            fs::metadata(layout.paths().record())
                                .unwrap()
                                .modified()
                                .unwrap(),
                            modified,
                            "reading historical evidence never rewrites it"
                        );
                        for replacement in [
                            encoded[..encoded.len() - 1].to_vec(),
                            [encoded.as_slice(), b"x"].concat(),
                            {
                                let mut changed = encoded.clone();
                                changed[0] ^= 1;
                                changed
                            },
                        ] {
                            fs::write(layout.paths().record(), replacement).unwrap();
                            assert!(saved.read(layout.paths(), &mut budget).is_err());
                            assert!(budget.check().is_ok(), "drift is not resource refusal");
                        }
                        fs::write(layout.paths().record(), &encoded).unwrap();
                        let alias = layout.paths().record().with_extension("alias");
                        fs::hard_link(layout.paths().record(), &alias).unwrap();
                        assert!(saved.read(layout.paths(), &mut budget).is_err());
                        fs::remove_file(&alias).unwrap();
                        fs::rename(layout.paths().record(), &alias).unwrap();
                        std::os::unix::fs::symlink(&alias, layout.paths().record()).unwrap();
                        assert!(saved.read(layout.paths(), &mut budget).is_err());
                        fs::remove_file(layout.paths().record()).unwrap();
                        fs::rename(&alias, layout.paths().record()).unwrap();
                        assert_eq!(saved.read(layout.paths(), &mut budget).unwrap(), encoded);
                        let record = crate::input::stored::Record::decode(&persisted, &mut budget)
                            .expect("persisted record frames under the original account");
                        let mut replay_request = Fixture::request(
                            &root,
                            resolution.retained_config.environment().clone(),
                        );
                        replay_request.universe =
                            Some(cargo::universe::Catalog::new(replay_definition));
                        let mut current_request_allocation = replay_request.allocation;
                        let catalog = replay_request.universe.as_ref().expect("current catalog");
                        let definition = catalog.current();
                        let placement = cargo::select_placement(
                            &replay_request,
                            catalog,
                            &cargo::render_manifest(&replay_request, None, &limits)?,
                            &limits,
                        )?;
                        let toolchain = cached.attestation.value().proof();
                        let current_context = definition.validate_context(
                            &replay_request.target,
                            &replay_request.profile,
                            toolchain.rustc_commit(),
                            toolchain.cargo_commit(),
                            replay_request.config.projection_schema(),
                            replay_request.toolchain.schemas().graph(),
                            replay_request.toolchain.schemas().unit(),
                            cargo::resolution::cold::cohort(
                                &replay_request,
                                layout.paths(),
                                &mut current_request_allocation,
                            )?,
                            replay_request.config.universe_context(),
                            replay_request.config.digest(),
                        )?;
                        let replay_paths = cargo::resolution::layout::Paths::observe(
                            layout.root().to_path_buf(),
                            store.graph_budget(),
                        )
                        .map_err(|source| Error::Node {
                            operation: "observe retained fixture paths",
                            source: Box::new(source),
                        })?;
                        let paths = replay_paths.final_form();
                        let metadata_bytes = record.metadata().len();
                        fs::remove_file(layout.paths().record()).unwrap();
                        let scope = cargo::resolution::content::Scope::complete(
                            layout.root(),
                            budget.graph(),
                        )
                        .unwrap();
                        let sealed = cargo::resolution::bundle::Stored::capture(
                            owned.take().expect("transfer reserved layout"),
                            &record,
                            &scope,
                            &mut budget,
                        )
                        .expect("complete cold record and inventory seal together");
                        let cargo_home = replay_request.config.cargo_home()?;
                        let mut compare = |budget: &mut crate::input::stored::Budget<'_>,
                                           cargo_limits: &Limits,
                                           cargo_allocation: &mut cargo::unit::Allocation| {
                            let lock = definition.candidate(
                                fs::read(paths.lock())
                                    .expect("current lock")
                                    .into_boxed_slice(),
                                &limits,
                            )?;
                            let observed = sealed.read_for_replay(budget)?;
                            replay_request.replay_observed_in(
                                &cached,
                                observed,
                                cargo_home.as_deref(),
                                definition,
                                lock,
                                placement.request,
                                current_context,
                                None,
                                None,
                                &limits,
                                cargo_limits,
                                &mut current_request_allocation,
                                cargo_allocation,
                            )
                        };
                        let replay =
                            compare(&mut budget, &cargo_limits, &mut resolution.cargo_allocation)
                                .expect("complete production replay joins without children");
                        assert_eq!(
                            replay.decoded.metadata.digest(),
                            resolution.metadata.digest()
                        );
                        assert_eq!(
                            replay.decoded.graph.digest(),
                            resolution.build_unit.digest()
                        );
                        assert!(replay.decoded.selected == resolution.selected);
                        assert_eq!(
                            replay.decoded.internal.package_id(),
                            resolution.internal.package_id()
                        );
                        replay
                            .input
                            .require_account(budget.graph())
                            .expect("completed inputs retain the original graph account");
                        assert!(matches!(
                            replay
                                .input
                                .require_account(&crate::node::GraphBudget::new()),
                            Err(crate::input::stored::Error::Encoding(
                                "completed input graph account differs"
                            ))
                        ));
                        let (schema, entries, fingerprint) = replay.input.observed_parts();
                        assert_eq!(schema, expected_input_schema);
                        assert!(entries == expected_input_entries);
                        assert_eq!(fingerprint, expected_input_fingerprint);
                        let replayed = replay.completion;
                        let cold = resolution.universe.expect("cold universe proof");
                        assert_eq!(replayed.proof().base(), cold.base());
                        assert_eq!(replayed.proof().complete(), cold.complete());
                        let replay_receipt =
                            serde_json::to_value(replayed.receipt().expect("replay receipt"))
                                .expect("replay receipt serializes");
                        assert_eq!(
                            replay_receipt, receipt,
                            "every receipt field matches cold completion"
                        );
                        let source = fs::read(paths.source()).expect("retained harness source");
                        fs::write(paths.source(), b"// changed harness input\n")
                            .expect("fixture source drift");
                        let drift =
                            compare(&mut budget, &cargo_limits, &mut resolution.cargo_allocation);
                        fs::write(paths.source(), source).expect("restore fixture source");
                        assert!(matches!(drift, Err(crate::input::stored::Error::Bundle(_))));
                        assert!(
                            budget.check().is_ok(),
                            "source drift is not resource refusal"
                        );
                        let ancestor = root.join("Cargo.toml");
                        assert!(!ancestor.exists());
                        fs::write(&ancestor, b"[workspace.package]\nversion='2.0.0'\n")
                            .expect("an external ancestor appears after original capture");
                        assert!(
                            compare(&mut budget, &cargo_limits, &mut resolution.cargo_allocation)
                                .is_err(),
                            "full replay regenerates ancestor coverage from current owners"
                        );
                        fs::remove_file(&ancestor).expect("restore ancestor absence");
                        assert!(
                            budget.check().is_ok(),
                            "workspace drift remains a clean miss"
                        );
                        compare(&mut budget, &cargo_limits, &mut resolution.cargo_allocation)
                            .expect("restored closure replays using the same original accounts");
                        // A Cargo JSON accounting target one byte under the
                        // metadata is recorded, never refused: replay still joins
                        // and nothing latches the original input account.
                        let mut tiny = cargo_limits;
                        tiny.json_bytes = metadata_bytes - 1;
                        let mut tiny_account = cargo::unit::Allocation::begin(&tiny);
                        let under_target = compare(&mut budget, &tiny, &mut tiny_account)
                            .expect("a JSON accounting target never withholds replay");
                        assert_eq!(
                            under_target.decoded.metadata.digest(),
                            resolution.metadata.digest()
                        );
                        assert!(budget.check().is_ok(), "a target crossing never latches");
                        // Only a genuine failure latches the original account, and
                        // the latch is terminal for every later read.
                        budget.refuse();
                        assert!(matches!(
                            budget.digest(b""),
                            Err(crate::input::stored::Error::Refused)
                        ));
                        fs::remove_file(replay_paths.record()).unwrap();
                        assert!(matches!(
                            sealed.read(&mut budget),
                            Err(crate::input::stored::Error::Refused)
                        ));
                        assert!(!replay_paths.record().exists());
                    }
                    Outcome::ProbeFailure => {
                        assert!(result.is_err());
                        assert!(!layout.receipt().exists());
                    }
                    Outcome::CorruptSidecar => {
                        assert!(matches!(
                            result.expect_err("invalid sidecar rejects completion"),
                            Error::Extraction {
                                code: "sidecar-schema",
                                ..
                            }
                        ));
                        assert!(!layout.receipt().exists());
                    }
                    Outcome::ConfigExceedance => {
                        let resolution = result?;
                        assert!(resolution.universe.is_some());
                        assert!(
                            layout.receipt().is_file(),
                            "the complete observation still publishes the assigned receipt"
                        );
                        // Config capture keeps every prior request usage on the
                        // one original account: the third retained path copy
                        // crossed by exactly the withheld byte, and capture's
                        // own later charges raised the truthful high water.
                        let limit = allocation.byte_limit as u64;
                        let record = allocation.exceedances();
                        let axis = record
                            .axis(cargo::REQUEST_BYTES_RESOURCE)
                            .expect("config capture records the crossing on the original account");
                        assert_eq!((axis.target, axis.first_requested), (limit, limit + 1));
                        assert_eq!(axis.high_water, allocation.bytes as u64);
                        assert!(axis.high_water > axis.first_requested);
                        assert!(allocation.items(0).is_ok(), "nothing latched");
                    }
                }
                Ok(())
            })
            .expect("exactly four Cargo commands and one Probe run under the existing owner")?;
        Ok(())
    });
    Fixture::retire_parts(root, store);
}

#[test]
fn full_cold_completion_publishes_only_the_assigned_receipt() {
    exercise(Outcome::Success);
}

#[test]
fn failed_probe_cannot_complete_or_publish_a_receipt() {
    exercise(Outcome::ProbeFailure);
}

#[test]
fn invalid_probe_output_cannot_complete_or_publish_a_receipt() {
    exercise(Outcome::CorruptSidecar);
}

#[test]
fn baseline_config_capture_keeps_all_prior_request_usage() {
    exercise(Outcome::ConfigExceedance);
}

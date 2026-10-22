use std::fs;

use rawr_import::metrics::{cell, evidence, report};
use sha2::{Digest as _, Sha256};

use super::{
    extend, write, Attempt, Complete, Context, Current, Document, Entry, Error, Execution, Facts,
    Key, Kind, Location,
};

fn context() -> Context {
    let build = cell::Build {
        executable: evidence::process::file::Identity {
            path: b"/fixture/rawr".as_slice().into(),
            digest: [4; 32],
            device: 1,
            inode: 2,
            size: 3,
        },
        source_tree: "committed-fixture".into(),
        lockfile_digest: [5; 32],
        features_digest: Sha256::digest(
            b"rawr.phase9.cell.features/v1\0default\0native-runner\0phase9-baseline",
        )
        .into(),
    };
    Context {
        corpus: [1; 32],
        fixture_digest: [2; 32],
        fixture: report::Fixture {
            name: "test fixture".into(),
            digest_sha256: evidence::hex(&[2; 32]).into(),
            package_list_fallbacks: 0,
        },
        environment: report::Environment {
            machine: "fixture".into(),
            operating_system: "fixture".into(),
            architecture: "fixture".into(),
            cpu: "fixture".into(),
            rustc: "fixture".into(),
            rustdoc: "fixture".into(),
            cargo: "fixture".into(),
            driver: "fixture".into(),
            target: "fixture".into(),
            profile: "release".into(),
        },
        provenance: report::Build {
            executable_path: "/fixture/rawr".into(),
            executable_digest_sha256: evidence::hex(&[4; 32]).into(),
            device: 1,
            inode: 2,
            size: 3,
            source_tree: "committed-fixture".into(),
            lockfile_digest_sha256: evidence::hex(&[5; 32]).into(),
            compiler: "fixture".into(),
            cargo: "fixture".into(),
            target: "fixture".into(),
            profile: "release".into(),
            features: vec!["import-baseline".into()].into_boxed_slice(),
        },
        build,
    }
}

fn key(scenario: report::Scenario) -> Key {
    Key {
        scenario,
        command: report::Command::Check,
    }
}

fn location(key: Key) -> Location {
    let name = if report::Scenario::CURRENT.contains(&key.scenario) {
        format!("/fixture/{:?}", key.command)
    } else {
        format!("/fixture/{:?}-{:?}", key.command, key.scenario)
    };
    Location {
        path: name.into_bytes().into_boxed_slice(),
        device: 1,
        inode: 3,
    }
}

fn executions(context: &Context, key: Key, location: &Location) -> Vec<Execution> {
    let roles = match key.scenario {
        report::Scenario::CleanCold => vec![cell::Role::RetainedCell],
        report::Scenario::ConcurrentCold => vec![cell::Role::RetainedCell; 2],
        _ => vec![cell::Role::Setup, cell::Role::RetainedCell],
    };
    roles
        .into_iter()
        .enumerate()
        .map(|(index, role)| {
            let profile = match (key.scenario, role) {
                (report::Scenario::ProfileRule, cell::Role::Setup) => cell::Profile::Baseline,
                (report::Scenario::ProfileRule, cell::Role::RetainedCell) => cell::Profile::Variant,
                _ => cell::Profile::Inactive,
            };
            let mut launch = [0; 32];
            launch[0] = key.scenario as u8 + 1;
            launch[1] = key.command as u8 + 1;
            launch[2] = index as u8 + 1;
            Execution {
                request: cell::Request::transform(
                    context.corpus,
                    launch,
                    1,
                    context.build.clone(),
                    cell::project::Identity {
                        path: location.path.clone(),
                        device: location.device,
                        inode: location.inode,
                        source_digest: [6; 32],
                        cache: cell::project::State {
                            bytes: 0,
                            digest: [7; 32],
                        },
                    },
                    context.fixture_digest,
                    key.command,
                    key.scenario,
                    role,
                    profile,
                    if role == cell::Role::Setup { 0 } else { 1 },
                )
                .expect("synthetic request is structurally valid"),
                capsule_digest: [index as u8 + 1; 32],
            }
        })
        .collect()
}

fn admit(
    context: &Context,
    key: Key,
    project: Location,
    executions: Vec<Execution>,
) -> Result<Entry, Error> {
    let kind = match key.scenario {
        report::Scenario::CorruptRunner | report::Scenario::CorruptResolution => Kind::Corruption,
        report::Scenario::ConcurrentCold => Kind::Paired,
        _ => Kind::Ordinary,
    };
    Entry::transform(
        context,
        &Current { key, project },
        kind,
        Facts {
            fixture: &context.fixture,
            environment: &context.environment,
            executions,
        },
        Document::bind(key, b"exact retained document\n"),
    )
}

fn entry(context: &Context, key: Key) -> Entry {
    let project = location(key);
    let executions = executions(context, key, &project);
    admit(context, key, project, executions).expect("valid row admission")
}

#[test]
fn complete_requires_all_fifty_five_exact_keys_without_duplicates() {
    let context = context();
    let mut entries: Vec<_> = report::Scenario::CURRENT
        .into_iter()
        .chain(report::Scenario::AFTER_CUTOVER)
        .flat_map(|scenario| report::Command::ALL.map(|command| Key { scenario, command }))
        .map(|key| entry(&context, key))
        .collect();
    assert_eq!(entries.len(), 55);
    assert!(Complete::transform(&context, &entries).is_ok());
    let mut other = self::context();
    other.environment.machine = "other-machine".into();
    assert!(Complete::transform(&other, &entries).is_err());
    entries.pop();
    assert!(Complete::transform(&context, &entries).is_err());
    entries.push(entry(&context, entries[0].key));
    assert!(Complete::transform(&context, &entries).is_err());
}

#[test]
fn actual_requests_must_match_the_common_corpus_build_fixture_and_project() {
    let context = context();
    let key = key(report::Scenario::CleanCold);
    let project = location(key);
    for change in 0..5 {
        let mut other = self::context();
        let mut other_project = project.clone();
        match change {
            0 => other.corpus = [9; 32],
            1 => other.build.executable.digest = [9; 32],
            2 => other.build.source_tree = "other-source".into(),
            3 => other.fixture_digest = [9; 32],
            4 => other_project.inode += 1,
            _ => unreachable!(),
        }
        let executions = executions(&other, key, &other_project);
        assert!(
            admit(&context, key, project.clone(), executions).is_err(),
            "change {change}"
        );
    }
    let mut facts = Facts {
        fixture: &context.fixture,
        environment: &context.environment,
        executions: executions(&context, key, &project),
    };
    let mut environment = context.environment.clone();
    environment.target = "different-target".into();
    facts.environment = &environment;
    assert!(Entry::transform(
        &context,
        &Current { key, project },
        Kind::Ordinary,
        facts,
        Document::bind(key, b"document")
    )
    .is_err());
}

#[test]
fn profile_setup_and_two_actual_pair_members_cannot_be_substituted() {
    let context = context();
    let profile = key(report::Scenario::ProfileRule);
    let project = location(profile);
    let mut facts = executions(&context, profile, &project);
    facts[1].request.core.profile = cell::Profile::Inactive;
    assert!(admit(&context, profile, project.clone(), facts).is_err());
    let mut facts = executions(&context, profile, &project);
    facts.remove(0);
    assert!(admit(&context, profile, project, facts).is_err());

    let pair = key(report::Scenario::ConcurrentCold);
    let project = location(pair);
    let facts = executions(&context, pair, &project);
    assert!(admit(&context, pair, project.clone(), facts[..1].to_vec()).is_err());
    assert!(admit(&context, pair, project.clone(), vec![facts[0].clone(); 2]).is_err());
    let mut changed = facts.clone();
    // A second valid request for the same root but a different cold prestate.
    let old = &facts[1].request.core;
    let mut other_project = old.project.clone();
    other_project.cache.digest = [8; 32];
    changed[1].request = cell::Request::transform(
        old.corpus_id,
        old.launch_id,
        old.launched_ns,
        old.build.clone(),
        other_project,
        old.fixture_digest,
        old.owner,
        old.scenario,
        old.role,
        old.profile,
        old.ordinal,
    )
    .unwrap();
    assert!(admit(&context, pair, project.clone(), changed).is_err());
    assert_eq!(
        admit(&context, pair, project, facts)
            .unwrap()
            .executions
            .len(),
        2
    );
}

#[test]
fn ordinary_lane_is_stable_and_specialized_rows_have_distinct_projects() {
    let context = context();
    let cold = entry(&context, key(report::Scenario::CleanCold));
    let warm_key = key(report::Scenario::IdenticalWarm);
    let warm = Current {
        key: warm_key,
        project: location(warm_key),
    };
    assert!(extend(std::slice::from_ref(&cold), &warm).is_ok());
    assert!(extend(
        std::slice::from_ref(&cold),
        &Current {
            key: cold.key,
            project: cold.project.clone(),
        }
    )
    .is_err());
    let mut changed = warm;
    changed.project.inode += 1;
    assert!(extend(std::slice::from_ref(&cold), &changed).is_err());
    assert!(extend(
        &[cold],
        &Current {
            key: key(report::Scenario::CorruptRunner),
            project: location(warm_key),
        }
    )
    .is_err());
}

#[test]
fn document_digest_binds_exact_bytes_and_exclusive_files_preserve_old_evidence() {
    let directory = std::env::temp_dir().join(format!(
        "rawr-corpus-document-{}-{}",
        std::process::id(),
        super::super::NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir(&directory).unwrap();
    let key = key(report::Scenario::CleanCold);
    let bytes = b"actual document\n";
    let document = Document::bind(key, bytes);
    let path = directory.join(document.name.as_ref());
    write(&path, bytes).unwrap();
    document.verify(&directory).unwrap();
    assert!(write(&path, b"replacement").is_err());
    assert_eq!(fs::read(&path).unwrap().as_slice(), bytes);
    fs::write(&path, b"changed document").unwrap();
    assert!(document.verify(&directory).is_err());
    fs::write(&path, b"actual document!").unwrap();
    assert_eq!(fs::metadata(&path).unwrap().len(), document.bytes);
    assert!(document.verify(&directory).is_err());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn failure_persistence_never_replaces_the_initiating_error() {
    let directory = std::env::temp_dir().join(format!(
        "rawr-corpus-failure-{}-{}",
        std::process::id(),
        super::super::NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir(&directory).unwrap();
    write(&directory.join("failure.json"), b"older evidence").unwrap();
    let attempt = Attempt {
        context: context(),
        directory: directory.clone(),
        entries: Vec::new(),
        current: None,
    };
    let failed = attempt.failed(Error::Binding("original refusal"));
    assert!(failed
        .to_string()
        .starts_with("complete-corpus observation is inconsistent: original refusal"));
    assert!(failed.to_string().contains("evidence persistence"));
    assert_eq!(
        fs::read(directory.join("failure.json")).unwrap().as_slice(),
        b"older evidence"
    );
    assert!(!directory.join("manifest.json").exists());
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn nested_ordinary_failure_retains_the_received_capsule_and_primary_refusal() {
    let directory = std::env::temp_dir().join(format!(
        "rawr-corpus-owner-failure-{}-{}",
        std::process::id(),
        super::super::NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    fs::create_dir(&directory).unwrap();
    let capsule = "{\"outcome\":\"failed\",\"counter\":17}\n";
    let source = super::super::Error::CellRetirement {
        source: Box::new(super::super::Error::OwnerObserved {
            source: Box::new(super::super::Error::Owner {
                command: report::Command::Run,
                reason: "original owner refusal".into(),
            }),
            evidence: capsule.into(),
        }),
        cleanup: Box::new(super::super::Error::Process {
            reason: "later cleanup failure".into(),
        }),
    };
    let attempt = Attempt {
        context: context(),
        directory: directory.clone(),
        entries: Vec::new(),
        current: None,
    };
    let failed = attempt.failed(Error::Owner(source));
    let value: serde_json::Value =
        serde_json::from_slice(&fs::read(directory.join("failure.json")).unwrap()).unwrap();
    assert_eq!(value["original"]["kind"], "retirement");
    assert_eq!(value["original"]["source"]["canonical_capsule"], capsule);
    assert_eq!(
        value["original"]["source"]["source"]["reason"],
        "original owner refusal"
    );
    assert!(failed.to_string().contains("original owner refusal"));
    assert!(!directory.join("manifest.json").exists());
    fs::remove_dir_all(directory).unwrap();
}

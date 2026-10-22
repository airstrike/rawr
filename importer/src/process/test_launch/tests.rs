use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use super::{Binding, Expected, Failure, File, FileLimits, Reply, Script, Step};
use crate::process::{self, Launcher};

struct Root(PathBuf);

impl Root {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "rawr-test-launch-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        Self(fs::canonicalize(root).unwrap())
    }

    fn script(&self, steps: Vec<Step>) -> Script {
        Script::new(
            self.0.clone(),
            FileLimits {
                count: 4,
                bytes: 1024,
            },
            steps,
        )
        .unwrap()
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn command() -> process::Command {
    process::Command::cargo(
        Vec::new(),
        process::CargoAction::Metadata,
        vec!["--offline".into(), "--format-version=1".into()],
        PathBuf::from("/fixture/project"),
        vec![("CARGO_HOME".into(), "/fixture/cargo-home".into())].into_boxed_slice(),
    )
    .unwrap()
}

fn step(reply: Reply) -> Step {
    Step::new(
        |_| {
            Expected::new(
                "/fixture/cargo".into(),
                command(),
                process::Limits::fixed(32, 16),
            )
        },
        |_| reply,
    )
}

fn launch(command: &process::Command) -> crate::Result<process::Output> {
    process::Cargo::fixture().run(command, process::Limits::fixed(32, 16))
}

#[test]
fn original_operation_error_precedes_only_unconsumed_later_children() {
    let root = Root::new();
    assert_eq!(
        root.script(vec![step(Reply::stdout(Vec::new()))])
            .run_result(|| Err::<(), _>("original refusal")),
        Ok(Err("original refusal"))
    );
    assert_eq!(
        root.script(vec![step(Reply::stdout(Vec::new()))])
            .run_result(|| Ok::<_, &'static str>(())),
        Err(Failure::Unconsumed)
    );
    assert_eq!(
        root.script(Vec::new()).run_result(|| {
            assert!(launch(&command()).is_err());
            Err::<(), _>("body after unexpected child")
        }),
        Err(Failure::Unexpected)
    );
}

#[test]
fn ordinary_native_replay_and_preflight_share_the_exact_boundary() {
    let root = Root::new();
    let native = process::Native {
        cargo: "/fixture/cargo".into(),
        rustc: "/fixture/rustc".into(),
        rustdoc: "/fixture/rustdoc".into(),
        rustup: "/fixture/rustup".into(),
        #[cfg(feature = "rustc-driver")]
        driver: "/fixture/driver".into(),
    };
    let preflight = || {
        process::Command::preflight(
            process::Preflight::version(process::Program::Rustc).unwrap(),
            PathBuf::from("/fixture/project"),
            Box::default(),
        )
    };
    let expected = preflight();
    root.script(vec![
        step(Reply::stdout(b"native".to_vec())),
        step(Reply::stdout(b"replay".to_vec())),
        Step::new(
            move |_| {
                Expected::new(
                    "/fixture/rustc".into(),
                    expected,
                    process::Limits::fixed(32, 16),
                )
            },
            |_| Reply::stdout(b"rustc".to_vec()),
        ),
    ])
    .run(|| {
        assert_eq!(
            native
                .production()
                .run(&command(), process::Limits::fixed(32, 16))
                .unwrap()
                .checked(process::Program::Cargo)
                .unwrap()
                .as_ref(),
            b"native"
        );
        assert_eq!(
            launch(&command())
                .unwrap()
                .checked(process::Program::Cargo)
                .unwrap()
                .as_ref(),
            b"replay"
        );
        assert_eq!(
            native
                .production()
                .run(&preflight(), process::Limits::fixed(32, 16))
                .unwrap()
                .checked(process::Program::Rustc)
                .unwrap()
                .as_ref(),
            b"rustc"
        );
    })
    .unwrap();
}

#[test]
fn empty_script_detects_any_child_and_unconsumed_steps_cannot_pass() {
    let root = Root::new();
    root.script(Vec::new()).run(|| {}).unwrap();
    assert_eq!(
        root.script(Vec::new()).run(|| {
            assert!(launch(&command()).is_err());
        }),
        Err(Failure::Unexpected)
    );
    assert_eq!(
        root.script(vec![step(Reply::stdout(Vec::new()))])
            .run(|| {}),
        Err(Failure::Unconsumed)
    );
}

#[test]
fn every_sealed_field_is_compared_before_response_and_failure_stays_latched() {
    let root = Root::new();
    for (case, field) in [
        "executable",
        "program",
        "kind",
        "arguments",
        "current directory",
        "environment",
        "stdout limit",
        "stderr limit",
    ]
    .into_iter()
    .enumerate()
    {
        let mut actual = command();
        let mut executable = PathBuf::from("/fixture/cargo");
        let mut limits = process::Limits::fixed(32, 16);
        match case {
            0 => executable.push("wrong"),
            1 => actual.program = process::Program::Rustc,
            2 => actual.kind = process::CommandKind::Cargo(process::CargoAction::PackageGit),
            3 => actual.arguments = vec!["metadata".into()].into_boxed_slice(),
            4 => actual.current_dir.push("wrong"),
            5 => actual.environment[0].1 = "changed".into(),
            6 => limits.stdout += 1,
            _ => limits.stderr += 1,
        }
        let expectation = Step::new(
            |_| {
                Expected::new(
                    "/fixture/cargo".into(),
                    command(),
                    process::Limits::fixed(32, 16),
                )
            },
            |_| panic!("a mismatched command must not construct response bytes"),
        );
        let result = root.script(vec![expectation]).run(|| {
            assert!(process::execute_production(&executable, &actual, limits).is_err());
            assert!(launch(&command()).is_err());
        });
        assert_eq!(result, Err(Failure::Mismatch(field)));
    }
}

#[test]
fn nested_reentrant_and_panicking_callbacks_cannot_escape_the_scope() {
    let root = Root::new();
    let nested = root.script(Vec::new());
    assert_eq!(
        root.script(Vec::new())
            .run(|| assert_eq!(nested.run(|| {}), Err(Failure::Nested))),
        Err(Failure::Nested)
    );
    let reentrant = Step::new(
        |_| {
            Expected::new(
                "/fixture/cargo".into(),
                command(),
                process::Limits::fixed(32, 16),
            )
        },
        |_| {
            assert!(launch(&command()).is_err());
            Reply::stdout(Vec::new())
        },
    );
    assert_eq!(
        root.script(vec![reentrant]).run(|| {
            assert!(launch(&command()).is_err());
        }),
        Err(Failure::Reentrant)
    );
    let panicking = Step::new(
        |_| panic!("fixture callback failure"),
        |_| Reply::stdout(Vec::new()),
    );
    assert_eq!(
        root.script(vec![panicking]).run(|| {
            assert!(std::panic::catch_unwind(|| {
                let _ = launch(&command());
            })
            .is_err());
            assert!(launch(&command()).is_err());
        }),
        Err(Failure::Panicked)
    );
    root.script(Vec::new()).run(|| {}).unwrap();
    assert!(super::execute(
        std::path::Path::new("/unused"),
        &command(),
        process::Limits::fixed(0, 0)
    )
    .is_none());
}

#[test]
fn bounded_streams_and_nonzero_exit_use_the_actual_output_contract() {
    let root = Root::new();
    root.script(vec![step(Reply::exit(
        7,
        vec![b'o'; 32],
        vec![b'e'; 16],
        Vec::new(),
    ))])
    .run(|| {
        assert!(matches!(
            launch(&command()).unwrap().checked(process::Program::Cargo),
            Err(crate::Error::Process {
                program: "cargo",
                ..
            })
        ));
    })
    .unwrap();
    for (stdout, stderr, resource) in [(33, 17, "child stdout"), (0, 17, "child stderr")] {
        root.script(vec![step(Reply::exit(0, vec![0; stdout], vec![0; stderr], vec![File::new("not-written".into(), vec![0])]))]).run(|| {
            assert!(matches!(launch(&command()), Err(crate::Error::Limit { resource: actual, .. }) if actual == resource));
        }).unwrap();
        assert!(!root.0.join("not-written").exists());
    }
}

#[test]
fn file_limits_are_cumulative_and_precede_all_effects() {
    let root = Root::new();
    let files = || vec![File::new("a".into(), b"123".to_vec())];
    Script::new(
        root.0.clone(),
        FileLimits { count: 2, bytes: 8 },
        vec![
            step(Reply::exit(0, Vec::new(), Vec::new(), files())),
            step(Reply::exit(0, Vec::new(), Vec::new(), files())),
        ],
    )
    .unwrap()
    .run(|| {
        launch(&command()).unwrap();
        launch(&command()).unwrap();
    })
    .unwrap();
    assert_eq!(fs::read(root.0.join("a")).unwrap(), b"123");
    for limits in [
        FileLimits { count: 0, bytes: 4 },
        FileLimits { count: 1, bytes: 3 },
    ] {
        let result = Script::new(
            root.0.clone(),
            limits,
            vec![step(Reply::exit(
                0,
                Vec::new(),
                Vec::new(),
                vec![File::new("b".into(), b"456".to_vec())],
            ))],
        )
        .unwrap()
        .run(|| {
            assert!(launch(&command()).is_err());
        });
        assert_eq!(result, Err(Failure::FileLimit));
        assert!(!root.0.join("b").exists());
    }
    for invalid in ["../escape", "/absolute", "a/../escape", "a/./b"] {
        let reply = Reply::exit(
            0,
            Vec::new(),
            Vec::new(),
            vec![
                File::new("first".into(), vec![1]),
                File::new(invalid.into(), vec![2]),
            ],
        );
        assert_eq!(
            root.script(vec![step(reply)]).run(|| {
                assert!(launch(&command()).is_err());
            }),
            Err(Failure::File("invalid relative path"))
        );
        assert!(!root.0.join("first").exists());
    }
}

#[test]
fn attempt_binding_changes_only_declared_path_positions_and_stays_immutable() {
    let root = Root::new();
    let store = root.0.join("store");
    let attempt = store
        .join("executions")
        .join("a".repeat(64))
        .join("b".repeat(64));
    for partition in ["generated", "build", "target", "declared"] {
        fs::create_dir_all(attempt.join(partition)).unwrap();
    }
    fs::write(attempt.join("lease"), b"lease").unwrap();
    let make = |attempt: &std::path::Path| {
        let mut command = command();
        command.arguments = vec![
            "metadata".into(),
            "--manifest-path".into(),
            attempt.join("generated/Cargo.toml").into_os_string(),
            "--config".into(),
            format!("build.build-dir={}", attempt.join("build").display()).into(),
        ]
        .into_boxed_slice();
        command
    };
    let first = Step::new(
        move |paths| {
            Expected::new(
                "/fixture/cargo".into(),
                make(paths.attempt()),
                process::Limits::fixed(32, 16),
            )
        },
        |_| Reply::stdout(Vec::new()),
    )
    .bind(Binding::argument(store, 2, "generated/Cargo.toml".into()));
    let second = Step::new(
        move |paths| {
            Expected::new(
                "/fixture/cargo".into(),
                make(paths.attempt()),
                process::Limits::fixed(32, 16),
            )
        },
        |_| Reply::stdout(Vec::new()),
    );
    assert_eq!(
        root.script(vec![first, second]).run(|| {
            launch(&make(&attempt)).unwrap();
            let mut wrong = make(&attempt);
            wrong.arguments[4] = "build.build-dir=/wrong".into();
            assert!(launch(&wrong).is_err());
        }),
        Err(Failure::Mismatch("arguments"))
    );
}

#[test]
fn binding_rejects_wrong_roots_and_preexisting_seals_before_expected_builder() {
    let root = Root::new();
    let store = root.0.join("store");
    let attempt = store
        .join("executions")
        .join("a".repeat(64))
        .join("b".repeat(64));
    for partition in ["generated", "build", "target", "declared"] {
        fs::create_dir_all(attempt.join(partition)).unwrap();
    }
    fs::write(attempt.join("lease"), b"lease").unwrap();
    let mut actual = command();
    actual.arguments =
        vec![attempt.join("generated/Cargo.toml").into_os_string()].into_boxed_slice();
    for (expected_store, expected) in [
        (
            root.0.join("other-store"),
            Failure::Binding("path outside expected executions"),
        ),
        (store, Failure::Binding("Attempt is already sealed")),
    ] {
        fs::write(attempt.join("seal"), b"sealed").unwrap();
        let step = Step::new(
            |_| panic!("invalid binding must not construct an expected command"),
            |_| Reply::stdout(Vec::new()),
        )
        .bind(Binding::argument(
            expected_store,
            0,
            "generated/Cargo.toml".into(),
        ));
        assert_eq!(
            root.script(vec![step]).run(|| {
                assert!(launch(&actual).is_err());
            }),
            Err(expected)
        );
    }
}

//! Exact legacy and fixed-Layout command expectations at the real launch seam.

use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::super::layout::Layout;
use super::{build, metadata, prefix, provisional_metadata, units};
use crate::cargo::{self, config, resolution, toolchain, Limits, Request};
use crate::context::Context;
use crate::node;
use crate::process::{self, test_launch, ToolPaths as _};
use crate::store::{self, bundle};
use crate::{Error, Result};

static NEXT: AtomicU64 = AtomicU64::new(0);

mod harness;

struct Fixture {
    root: PathBuf,
    request: Request,
    context: Context,
    store: store::Store,
}

impl Fixture {
    fn open() -> Self {
        Self::open_with(&Limits::standard())
    }

    /// Open a fixture whose store context captures a configured limits profile.
    fn open_with(limits: &Limits) -> Self {
        Self::open_at(limits, None)
    }

    /// Open a fixture at an exact root a previous fixture used and removed,
    /// or at a fresh unique root when `root` is `None`. Reusing one root lets
    /// two limits profiles run over byte-identical paths, so their semantic
    /// output is comparable across target settings: every path the
    /// contract, native proof, request key and node contents embed is the
    /// same, and only the limits profile differs.
    fn open_at(limits: &Limits, root: Option<&Path>) -> Self {
        let root = match root {
            Some(root) => {
                fs::create_dir(root).expect("a reused fixture root is recreated exclusively");
                root.to_path_buf()
            }
            None => Self::unique_root(),
        };
        Self::open_root(limits, root)
    }

    fn unique_root() -> PathBuf {
        // A unique root per fixture: the process ID alone is reused across
        // runs, and a crashed earlier run can leave its directory behind.
        loop {
            let nanos = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |elapsed| elapsed.as_nanos());
            let root = std::env::temp_dir().join(format!(
                "rawr-cold-commands-{{cargo-cache-home}}-{{literal}}-{}-{}-{nanos}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed),
            ));
            match fs::create_dir(&root) {
                Ok(()) => break root,
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(error) => panic!("fixture root creates: {error}"),
            }
        }
    }

    fn open_root(limits: &Limits, root: PathBuf) -> Self {
        let root = fs::canonicalize(root).expect("fixture root canonicalizes");
        let project = root.join("project");
        let home = root.join("cargo-home");
        let authority = root.join("authority");
        fs::create_dir(&project).expect("project without hierarchical config creates");
        fs::create_dir_all(authority.join(".cargo")).expect("authority config directory creates");
        fs::create_dir(&home).expect("Cargo home creates");
        fs::write(
            authority.join("Cargo.toml"),
            b"[workspace]\nresolver = \"3\"\n",
        )
        .expect("selected Cargo authority manifest publishes");
        fs::write(
            authority.join(".cargo/config.toml"),
            b"[net]\noffline = true\n",
        )
        .expect("authority-only hierarchical config publishes");
        // The user's target directory holds the project's store; the
        // cold commands still override it with the layout's own.
        let overridden = root.join("overridden-target");
        let environment = config::Environment::from_pairs([
            (
                "CARGO_HOME",
                home.to_str().expect("fixture home is Unicode"),
            ),
            ("CARGO_BUILD_JOBS", "999"),
            ("NUM_JOBS", "999"),
            (
                "CARGO_TARGET_DIR",
                overridden.to_str().expect("fixture target is Unicode"),
            ),
        ])
        .expect("fixture environment seals");
        let request = Self::request(&root, environment.clone());
        assert!(!request.config.explicit_configs().is_empty());
        assert_eq!(
            request.config.explicit_configs(),
            &[authority.join(".cargo/config.toml")]
        );
        let context = Context::capture_config(&project, &request.config, toolchain::HOST, limits)
            .expect("store context captures");
        toolchain::Attestation::fixture_current(&context, &root.join("toolchain"));
        let store = context.open_store().expect("actual shared store opens");
        Self {
            root,
            request,
            context,
            store,
        }
    }

    fn request(root: &Path, environment: config::Environment) -> Request {
        let limits = Limits::standard();
        let project = root.join("project");
        let authority = root.join("authority");
        let dependency =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/catalog-api");
        Request::builder(&project, &project.join("target/rawr/cache"), &limits)
            .expect("request builder seals")
            .environment(environment)
            .cargo_authority(&authority.join("Cargo.toml"))
            .policy(crate::extract::Policy::manifest().seal())
            .dependency(
                config::Dependency::path("catalog", &dependency)
                    .and_then(|dependency| dependency.package("catalog-api"))
                    .expect("dependency seals"),
            )
            .build()
            .expect("the real request seals")
    }

    fn reserve(&self, operation: impl FnOnce(&Layout, &process::Native) -> Result<()>) {
        Self::reserve_parts(&self.context, &self.store, operation);
    }

    fn reserve_parts(
        context: &Context,
        store: &store::Store,
        operation: impl FnOnce(&Layout, &process::Native) -> Result<()>,
    ) {
        Self::reserve_owned_parts(context, store, |layout, native| operation(&layout, native));
    }

    fn reserve_owned_parts(
        context: &Context,
        store: &store::Store,
        operation: impl FnOnce(Layout, &process::Native) -> Result<()>,
    ) {
        let attestation = context
            .cached_toolchain()
            .expect("fixture toolchain is retained");
        let native = process::Native::from_attestation(&attestation);
        let recipe = context
            .discover::<resolution::BuildProduct>(b"rawr.cold-command-test/v1", 4096)
            .expect("reservation recipe derives");
        store
            .with_ownership(|ownership| {
                store.with_heavy_lease(|| {
                    store.with_execution_lease(recipe.execution(), || {
                        store.with_execution_bundle_reservation(
                            ownership,
                            &recipe,
                            None,
                            |attempt| {
                                let layout = Layout::bind(attempt, store.graph_budget())?;
                                operation(layout, &native)
                                    .map(store::Reserved::Reuse)
                                    .map_err(node::Error::from)
                            },
                        )
                    })
                })
            })
            .expect("direct commands run under the existing outer ownership");
    }

    fn retire(self) {
        Self::retire_parts(self.root, self.store);
    }

    fn retire_parts(root: PathBuf, store: store::Store) {
        assert_eq!(store.fixture_usage().expect("ledger reads").3, 0);
        assert_eq!(
            bundle::count_bundles(store.root()).expect("bundles count"),
            0
        );
        store
            .fixture_physical_inventory()
            .expect("physical accounting agrees");
        store.retire_fixture().expect("fixture store retires");
        fs::remove_dir_all(root).expect("fixture root retires");
    }
}

#[derive(Clone, Copy)]
enum Command {
    Metadata(bool),
    Units,
    Build,
}

fn environment(
    request: &Request,
    native: &process::Native,
    target: &Path,
) -> Box<[(OsString, OsString)]> {
    let production = native.production();
    let (rustc, rustdoc) = production.cargo_tools();
    let home = request
        .config
        .environment()
        .entries()
        .find(|(key, _)| *key == "CARGO_HOME")
        .expect("fixture home is sealed")
        .1;
    let mut values: Vec<(OsString, OsString)> = vec![
        ("CARGO_HOME".into(), OsString::from(home)),
        ("CARGO_INCREMENTAL".into(), "0".into()),
        ("CARGO_TARGET_DIR".into(), target.as_os_str().to_owned()),
        ("RUSTC".into(), rustc.as_os_str().to_owned()),
        ("RUSTDOC".into(), rustdoc.as_os_str().to_owned()),
    ];
    values.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    values.into_boxed_slice()
}

fn expected(
    request: &Request,
    native: &process::Native,
    paths: &super::super::layout::Cargo,
    kind: Command,
    bound: bool,
) -> test_launch::Step {
    expected_reply(request, native, paths, kind, bound, |_| {
        test_launch::Reply::stdout(b"exact Cargo bytes".to_vec())
    })
}

fn expected_reply(
    request: &Request,
    native: &process::Native,
    paths: &super::super::layout::Cargo,
    kind: Command,
    bound: bool,
    reply: impl FnOnce(&test_launch::Paths) -> test_launch::Reply + 'static,
) -> test_launch::Step {
    let mut prefix = Vec::new();
    for path in request.config.explicit_configs() {
        prefix.push(OsString::from("--config"));
        prefix.push(path.as_os_str().to_owned());
    }
    if bound {
        prefix.extend(["--config".into(), paths.config().as_os_str().to_owned()]);
    }
    let (action, tail) = match kind {
        Command::Metadata(locked) => {
            let mut tail = vec![
                "--format-version".into(),
                request.toolchain.schemas().graph().to_string().into(),
                "--manifest-path".into(),
                paths.manifest().as_os_str().to_owned(),
                "--filter-platform".into(),
                request.target.as_ref().into(),
            ];
            if locked {
                tail.push("--locked".into());
            }
            (process::CargoAction::Metadata, tail)
        }
        Command::Units => {
            prefix.extend(["-Z".into(), "unstable-options".into()]);
            (
                process::CargoAction::UnitBuild(request.parallelism),
                vec![
                    "--unit-graph".into(),
                    "--manifest-path".into(),
                    paths.manifest().as_os_str().to_owned(),
                    "--target".into(),
                    request.target.as_ref().into(),
                    "--profile".into(),
                    request.profile.as_ref().into(),
                    "--locked".into(),
                ],
            )
        }
        Command::Build => (
            {
                if bound {
                    let build = crate::location::rawr_directory(
                        &request.project_root,
                        request.config.environment(),
                    )
                    .join("build");
                    let shared = crate::deps::directory(&build).unwrap();
                    prefix.extend(super::shared_arguments(&shared));
                }
                process::CargoAction::AuthoritativeBuild(request.parallelism)
            },
            vec![
                "--message-format".into(),
                "json-render-diagnostics".into(),
                "--manifest-path".into(),
                paths.manifest().as_os_str().to_owned(),
                "--target".into(),
                request.target.as_ref().into(),
                "--profile".into(),
                request.profile.as_ref().into(),
                "--locked".into(),
            ],
        ),
    };
    let command = process::Command::cargo(
        prefix,
        action,
        tail,
        request.project_root.clone(),
        environment(request, native, paths.target()),
    )
    .expect("independent expected command seals");
    let executable = native
        .production()
        .executable(process::Program::Cargo)
        .to_path_buf();
    let config = bound.then(|| (paths.config().to_path_buf(), paths.config_bytes()));
    test_launch::Step::new(
        move |_| {
            if let Some((path, bytes)) = config {
                assert_eq!(
                    fs::read(path).expect("assigned config exists at launch"),
                    bytes
                );
            }
            test_launch::Expected::new(executable, command, Limits::standard().process_limits())
        },
        reply,
    )
}

fn script(layout: &Layout, steps: Vec<test_launch::Step>) -> test_launch::Script {
    test_launch::Script::new(
        layout.root().to_path_buf(),
        test_launch::FileLimits { count: 0, bytes: 0 },
        steps,
    )
    .expect("the exact launch script has no file effects")
}

#[test]
fn fixed_commands_share_legacy_construction_and_preserve_exact_build_environment() {
    let fixture = Fixture::open();
    fixture.reserve(|layout, native| {
        let request = &fixture.request;
        let limits = Limits::standard();
        let production = native.production();
        let steps = vec![
            expected(
                request,
                native,
                layout.provisional(),
                Command::Metadata(false),
                true,
            ),
            expected(
                request,
                native,
                layout.final_form(),
                Command::Metadata(false),
                true,
            ),
            expected(
                request,
                native,
                layout.final_form(),
                Command::Metadata(true),
                true,
            ),
            expected(
                request,
                native,
                layout.final_form(),
                Command::Metadata(true),
                false,
            ),
            expected(request, native, layout.final_form(), Command::Units, true),
            expected(request, native, layout.final_form(), Command::Units, false),
            expected(request, native, layout.final_form(), Command::Build, true),
            expected(request, native, layout.final_form(), Command::Build, false),
        ];
        let sentinel = layout.final_form().target().join("keep");
        fs::write(&sentinel, b"existing output").expect("target sentinel publishes");
        script(layout, steps)
            .run(|| -> Result<()> {
                for bytes in [
                    provisional_metadata(&production, request, layout, &limits)?,
                    metadata(&production, request, layout, false, &limits)?,
                    metadata(&production, request, layout, true, &limits)?,
                    cargo::cargo_metadata(
                        &production,
                        request,
                        layout.final_form().manifest(),
                        layout.final_form().target(),
                        true,
                        &limits,
                    )?,
                    units(&production, request, layout, &limits)?,
                    cargo::cargo_units(
                        &production,
                        request,
                        layout.final_form().manifest(),
                        layout.final_form().target(),
                        &limits,
                    )?,
                ] {
                    assert_eq!(bytes.as_ref(), b"exact Cargo bytes");
                }
                let (bytes, retained) = build(
                    &production,
                    request,
                    layout,
                    request
                        .config
                        .command_environment(layout.final_form().target(), &production)?,
                    &limits,
                )?;
                assert_eq!(bytes.as_ref(), b"exact Cargo bytes");
                assert_eq!(
                    retained,
                    environment(request, native, layout.final_form().target())
                );
                let legacy = cargo::cargo_execute_authoritative_build(
                    &production,
                    request,
                    layout.final_form().manifest(),
                    retained,
                    &limits,
                )?;
                assert_eq!(legacy.as_ref(), b"exact Cargo bytes");
                Ok(())
            })
            .expect("all commands match under one outer execution lease")?;
        assert_eq!(
            fs::read(sentinel).expect("target is not cleared"),
            b"existing output"
        );
        Ok(())
    });
    fixture.retire();
}

#[test]
fn relative_configs_keep_braced_ancestors_and_reject_overwritten_controls() {
    let fixture = Fixture::open();
    fixture.reserve(|layout, native| {
        assert!(layout
            .root()
            .to_str()
            .unwrap()
            .contains("{cargo-cache-home}"));
        for (paths, relative) in [
            (layout.provisional(), "build/provisional"),
            (layout.final_form(), "build/final"),
        ] {
            let bytes = fs::read(paths.config()).expect("reserved config reads");
            assert_eq!(
                bytes,
                format!("[build]\nbuild-dir = \"{relative}\"\n").as_bytes()
            );
            let document: toml_edit::DocumentMut =
                std::str::from_utf8(&bytes).unwrap().parse().unwrap();
            let value = document["build"]["build-dir"].as_str().unwrap();
            assert!(!value.contains(['{', '}']));
            // Pinned Cargo 59800466: context/path.rs:46–75 checks template
            // braces before joining Definition::Cli(Some(path)).root;
            // context/value.rs:108–112 chooses config.parent().parent().
            let anchor = paths.config().parent().unwrap().parent().unwrap();
            assert_eq!(anchor, layout.root());
            assert_eq!(anchor.join(value), paths.build());
        }
        let changed = layout.final_form().config();
        fs::write(changed, b"[build]\nbuild-dir = \"build/wrong\"\n")
            .expect("mutable control corrupts");
        let error = script(layout, Vec::new())
            .run(|| {
                metadata(
                    &native.production(),
                    &fixture.request,
                    layout,
                    true,
                    &Limits::standard(),
                )
            })
            .expect("changed control cannot launch a child")
            .expect_err("changed control rejects");
        assert!(matches!(
            error,
            Error::Node {
                operation: "reserved Cargo build configuration",
                ..
            }
        ));
        assert_eq!(
            fs::read(changed).unwrap(),
            b"[build]\nbuild-dir = \"build/wrong\"\n"
        );
        Ok(())
    });
    fixture.retire();
}

#[cfg(unix)]
#[test]
fn config_arguments_preserve_non_utf8_native_path_bytes() {
    use std::os::unix::ffi::OsStringExt as _;
    let fixture = Fixture::open();
    let path = PathBuf::from(OsString::from_vec(b"/fixture/config-\xff.toml".to_vec()));
    let arguments = prefix(&fixture.request, &path).expect("native config path copies exactly");
    assert_eq!(arguments.last().unwrap().as_os_str(), path.as_os_str());
    fixture.retire();
}

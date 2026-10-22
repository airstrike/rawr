use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use rawr_import::cache::{self, Decision, Undeclared};
use rawr_import::cargo::config::{Dependency, Environment};
use rawr_import::cargo::{Limits, Request};
use rawr_import::extract::Policy;
use rawr_import::process::Native;

static TEST_ID: AtomicU64 = AtomicU64::new(1);

struct Project(PathBuf);

impl Project {
    fn new(label: &str) -> Self {
        rawr_import::store::isolate_for_tests();
        let path = std::env::temp_dir().join(format!(
            "rawr-freshness-{label}-{}-{}",
            std::process::id(),
            TEST_ID.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&path).expect("the unique freshness project is created");
        Self(fs::canonicalize(path).expect("the freshness project is canonical"))
    }

    fn cache(&self) -> PathBuf {
        self.0.join(".rawr/cache")
    }
}

impl Drop for Project {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn declared_build_inputs_and_default_fallback_publish_deterministically() {
    let _turn = rawr_import::store::test_turn();
    let project = Project::new("determinism");
    let limits = Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    let first = extract(&native, &project, environment("set"), &limits)
        .expect("the first freshness graph publishes");
    let second = extract(&native, &project, environment("set"), &limits)
        .expect("the repeated freshness graph publishes");

    assert_canonical_equal(first.canonical_bytes(), second.canonical_bytes());
    assert_eq!(first.fingerprint(), second.fingerprint());
    assert_eq!(
        first.freshness_fingerprint(),
        second.freshness_fingerprint()
    );

    let changed = extract(&native, &project, environment("changed"), &limits)
        .expect("a changed declared environment publishes a new graph");
    let unset = extract(&native, &project, environment_without_freshness(), &limits)
        .expect("an unset declared environment publishes a new graph");
    assert_ne!(first.fingerprint(), changed.fingerprint());
    assert_ne!(changed.fingerprint(), unset.fingerprint());
    assert_ne!(
        first.freshness_fingerprint(),
        changed.freshness_fingerprint()
    );
    assert_ne!(
        changed.freshness_fingerprint(),
        unset.freshness_fingerprint()
    );
    assert_ne!(first.freshness_fingerprint(), unset.freshness_fingerprint());
}

#[test]
fn authoritative_build_captures_out_dir_semantics_once_and_never_reuses_sidecars() {
    let _turn = rawr_import::store::test_turn();
    let project = Project::new("authoritative-build");
    let copied = project.0.join("freshness-copy");
    copy_directory(&fixture("freshness-api"), &copied);
    let source = copied.join("src/lib.rs");
    let mut contents = fs::read_to_string(&source).expect("the copied source is readable");
    contents.push_str(
        "\npub const IMPORT_OUT_DIR: &str = env!(\"OUT_DIR\");\n\
         pub const IMPORT_OUT_DIR_LEN: usize = env!(\"OUT_DIR\").len();\n",
    );
    fs::write(&source, contents).expect("the public OUT_DIR regression can be installed");
    let limits = Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    let environment = dependency_environment(&copied, "set")
        .setting("FRESH_EMBED_OUT_DIR", "1")
        .expect("the generated OUT_DIR semantic seam is sealed");
    let first = extract_dependency(&native, &project, &copied, environment.clone(), &limits)
        .expect("the authoritative Build publishes direct and generated OUT_DIR semantics");
    let second = extract_dependency(&native, &project, &copied, environment, &limits)
        .expect("a fresh authoritative Build republishes every required sidecar");

    assert_eq!(first.invocation_count(), 1);
    assert_eq!(second.invocation_count(), 1);
    assert_canonical_equal(first.canonical_bytes(), second.canonical_bytes());
    assert_eq!(first.fingerprint(), second.fingerprint());
    assert_eq!(
        first.freshness_fingerprint(),
        second.freshness_fingerprint()
    );
    let canonical =
        std::str::from_utf8(first.canonical_bytes()).expect("the canonical graph is UTF-8 JSON");
    assert!(canonical.contains("IMPORT_OUT_DIR"));
    assert!(canonical.contains("authoritative-target"));
}

#[test]
fn declared_path_shapes_each_change_the_freshness_fingerprint() {
    let _turn = rawr_import::store::test_turn();
    let project = Project::new("path-shapes");
    let copied = project.0.join("freshness-copy");
    copy_directory(&fixture("freshness-api"), &copied);
    let limits = Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    let mut previous = extract_dependency(
        &native,
        &project,
        &copied,
        dependency_environment(&copied, "set"),
        &limits,
    )
    .expect("the initial declared paths publish");

    for (path, contents) in [
        ("relative.txt", b"changed relative source\n".as_slice()),
        (
            "watched/value.txt",
            b"changed directory source\n".as_slice(),
        ),
        ("absolute.txt", b"changed absolute source\n".as_slice()),
        ("missing.txt", b"created missing source\n".as_slice()),
    ] {
        fs::write(copied.join(path), contents).expect("the copied declared input can mutate");
        let current = extract_dependency(
            &native,
            &project,
            &copied,
            dependency_environment(&copied, "set"),
            &limits,
        )
        .expect("the changed declared input publishes");
        assert_eq!(previous.fingerprint(), current.fingerprint());
        assert_ne!(
            previous.freshness_fingerprint(),
            current.freshness_fingerprint(),
            "declared input `{path}` must alter freshness"
        );
        previous = current;
    }

    fs::create_dir(copied.join("watched/empty")).expect("an empty declared directory can be added");
    let current = extract_dependency(
        &native,
        &project,
        &copied,
        dependency_environment(&copied, "set"),
        &limits,
    )
    .expect("the declared empty-directory topology publishes");
    assert_ne!(
        previous.freshness_fingerprint(),
        current.freshness_fingerprint(),
        "empty declared directories must alter freshness"
    );
}

#[test]
fn generated_out_directory_mutation_cannot_construct_extracted_graph() {
    let _turn = rawr_import::store::test_turn();
    let project = Project::new("generated-output");
    let copied = project.0.join("freshness-copy");
    copy_directory(&fixture("freshness-api"), &copied);
    let limits = Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    let resolution = single_request(
        &project,
        &copied,
        dependency_environment(&copied, "set"),
        &limits,
    )
    .resolve(&native, &limits)
    .expect("the generated-output resolution seals");
    let generated = only_named_file(
        &project.cache().join("resolution/authoritative-target"),
        "generated.rs",
    );
    fs::write(
        generated,
        b"pub const GENERATED: &str = \"changed output\";\n",
    )
    .expect("the copied OUT_DIR source can mutate");
    let error = rawr_import::extract::from_resolution(resolution, &native, &limits)
        .err()
        .expect("changed generated output cannot publish an extracted graph");
    assert!(format!("{error}").contains("input-changed"));
}

#[test]
fn cargo_process_environment_ignores_overridden_callers_and_retains_set_unset() {
    let _turn = rawr_import::store::test_turn();
    let project = Project::new("environment");
    let limits = Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    let first_environment = environment("set")
        .setting("RUSTC", "/caller/ignored-rustc")
        .and_then(|environment| environment.setting("RUSTDOC", "/caller/ignored-rustdoc"))
        .and_then(|environment| environment.setting("CARGO_TARGET_DIR", "/caller/ignored-target"))
        .and_then(|environment| environment.setting("CARGO_INCREMENTAL", "1"))
        .and_then(|environment| environment.removing("TARGET"))
        .and_then(|environment| environment.removing("PROFILE"))
        .and_then(|environment| environment.removing("CARGO_CFG_TARGET_OS"))
        .and_then(|environment| environment.removing("CARGO_FEATURE_NONEXISTENT"))
        .expect("the first reserved environment seals");
    let second_environment = environment("set")
        .setting("RUSTC", "/different/ignored-rustc")
        .and_then(|environment| environment.setting("RUSTDOC", "/different/ignored-rustdoc"))
        .and_then(|environment| environment.setting("CARGO_TARGET_DIR", "/different/target"))
        .and_then(|environment| environment.setting("CARGO_INCREMENTAL", "99"))
        .and_then(|environment| environment.removing("TARGET"))
        .and_then(|environment| environment.removing("PROFILE"))
        .and_then(|environment| environment.removing("CARGO_CFG_TARGET_OS"))
        .and_then(|environment| environment.removing("CARGO_FEATURE_NONEXISTENT"))
        .expect("the second reserved environment seals");
    let first = extract(&native, &project, first_environment, &limits)
        .expect("the first normalized process environment publishes");
    let second = extract(&native, &project, second_environment, &limits)
        .expect("the second normalized process environment publishes");
    assert_eq!(first.fingerprint(), second.fingerprint());
    assert_eq!(
        first.freshness_fingerprint(),
        second.freshness_fingerprint()
    );

    let target = extract(
        &native,
        &project,
        environment("set")
            .setting("TARGET", "caller-value")
            .and_then(|environment| environment.setting("PROFILE", "caller-profile"))
            .and_then(|environment| environment.setting("CARGO_CFG_TARGET_OS", "caller-target-os"))
            .and_then(|environment| {
                environment.setting("CARGO_FEATURE_NONEXISTENT", "caller-feature")
            })
            .expect("the caller TARGET environment seals"),
        &limits,
    )
    .expect("the caller TARGET process environment publishes");
    assert_eq!(first.fingerprint(), target.fingerprint());
    assert_ne!(
        first.freshness_fingerprint(),
        target.freshness_fingerprint()
    );
}

#[test]
fn pinned_cargo_package_file_set_honors_include_and_exclude() {
    let _turn = rawr_import::store::test_turn();
    let project = Project::new("package-files");
    let copied = project.0.join("fallback-copy");
    copy_directory(&fixture("fallback-api"), &copied);
    let limits = Limits::standard();
    let native = Native::pinned().expect("the pinned driver toolchain is installed");
    let first = extract_fallback(&native, &project, &copied, &limits)
        .expect("the initial exact package file set publishes");
    fs::write(copied.join("excluded.txt"), b"changed excluded source\n")
        .expect("the excluded package file can mutate");
    fs::write(
        copied.join(".cargo_vcs_info.json"),
        b"{\"git\":{\"sha1\":\"1111111111111111111111111111111111111111\"}}\n",
    )
    .expect("the excluded real VCS archive-name file can mutate");
    fs::write(
        copied.join("Cargo.toml.orig"),
        b"# changed excluded real original-manifest archive entry\n",
    )
    .expect("the excluded real original-manifest archive-name file can mutate");
    fs::write(
        copied.join("Cargo.lock"),
        b"# changed but valid excluded real lock archive entry\nversion = 4\n\n[[package]]\nname = \"fallback-api\"\nversion = \"0.1.0\"\n",
    )
    .expect("the excluded real lock archive-name file can mutate");
    let excluded = extract_fallback(&native, &project, &copied, &limits)
        .expect("the excluded package mutation publishes");
    assert_eq!(first.fingerprint(), excluded.fingerprint());
    assert_eq!(
        first.freshness_fingerprint(),
        excluded.freshness_fingerprint()
    );

    fs::write(copied.join("included.txt"), b"changed included source\n")
        .expect("the included package file can mutate");
    let included = extract_fallback(&native, &project, &copied, &limits)
        .expect("the included package mutation publishes");
    assert_eq!(first.fingerprint(), included.fingerprint());
    assert_ne!(
        first.freshness_fingerprint(),
        included.freshness_fingerprint()
    );

    let added_project = Project::new("package-member-addition");
    let added_copy = added_project.0.join("fallback-copy");
    copy_directory(&fixture("fallback-api"), &added_copy);
    let resolution = Request::builder(&added_project.0, &added_project.cache(), &limits)
        .expect("the limits-bound request builder seals")
        .environment(
            Environment::capture()
                .and_then(|environment| environment.setting("CARGO_NET_OFFLINE", "true"))
                .expect("the package-member environment seals"),
        )
        .policy(Policy::manifest(&limits).seal())
        .expect("the manifest policy fits the request")
        .dependency(
            Dependency::path("fallback", &added_copy, &limits)
                .and_then(|dependency| dependency.package("fallback-api"))
                .expect("the fallback member-addition dependency seals"),
        )
        .expect("the fallback dependency fits the request")
        .build()
        .expect("the member-addition request seals")
        .resolve(&native, &limits)
        .expect("the authoritative Build captures the package member set");
    fs::write(
        added_copy.join("src/newly-added.rs"),
        "pub const ADDED: u32 = 1;\n",
    )
    .expect("a new included package member can be added after capture");
    let error = rawr_import::extract::from_resolution(resolution, &native, &limits)
        .err()
        .expect("a newly-added included fallback member rejects publication");
    assert!(format!("{error}").contains("package-file-set-changed"));
}

#[test]
fn undeclared_external_state_never_yields_a_warm_reuse_decision() {
    let _turn = rawr_import::store::test_turn();
    assert_eq!(
        cache::undeclared_external(Undeclared::InvokeCargo)
            .expect("the conservative policy can delegate"),
        Decision::InvokeCargo
    );
    let error = cache::undeclared_external(Undeclared::Reject)
        .expect_err("the strict policy rejects non-hermetic state");
    assert!(format!("{error}").contains("non-hermetic-external-state"));
}

fn extract(
    native: &Native,
    project: &Project,
    environment: Environment,
    limits: &Limits,
) -> rawr_import::Result<rawr_import::api::Graph<rawr_import::api::Extracted>> {
    request(project, environment, limits)
        .resolve(native, limits)
        .and_then(|resolution| rawr_import::extract::from_resolution(resolution, native, limits))
}

fn extract_dependency(
    native: &Native,
    project: &Project,
    dependency: &Path,
    environment: Environment,
    limits: &Limits,
) -> rawr_import::Result<rawr_import::api::Graph<rawr_import::api::Extracted>> {
    single_request(project, dependency, environment, limits)
        .resolve(native, limits)
        .and_then(|resolution| rawr_import::extract::from_resolution(resolution, native, limits))
}

fn request(project: &Project, environment: Environment, limits: &Limits) -> Request {
    Request::builder(&project.0, &project.cache(), limits)
        .expect("the limits-bound request builder seals")
        .environment(environment)
        .policy(Policy::manifest(limits).seal())
        .expect("the manifest policy fits the request")
        .dependency(
            Dependency::path("freshness", &fixture("freshness-api"), limits)
                .and_then(|dependency| dependency.package("freshness-api"))
                .expect("the freshness dependency seals"),
        )
        .expect("the freshness dependency fits the request")
        .dependency(
            Dependency::path("fallback", &fixture("fallback-api"), limits)
                .and_then(|dependency| dependency.package("fallback-api"))
                .expect("the fallback dependency seals"),
        )
        .expect("the fallback dependency fits the request")
        .build()
        .expect("the freshness request seals")
}

fn single_request(
    project: &Project,
    dependency: &Path,
    environment: Environment,
    limits: &Limits,
) -> Request {
    Request::builder(&project.0, &project.cache(), limits)
        .expect("the limits-bound request builder seals")
        .environment(environment)
        .policy(Policy::manifest(limits).seal())
        .expect("the manifest policy fits the request")
        .dependency(
            Dependency::path("freshness", dependency, limits)
                .and_then(|dependency| dependency.package("freshness-api"))
                .expect("the copied freshness dependency seals"),
        )
        .expect("the freshness dependency fits the request")
        .build()
        .expect("the copied freshness request seals")
}

fn extract_fallback(
    native: &Native,
    project: &Project,
    dependency: &Path,
    limits: &Limits,
) -> rawr_import::Result<rawr_import::api::Graph<rawr_import::api::Extracted>> {
    Request::builder(&project.0, &project.cache(), limits)?
        .environment(Environment::capture()?.setting("CARGO_NET_OFFLINE", "true")?)
        .policy(Policy::manifest(limits).seal())?
        .dependency(Dependency::path("fallback", dependency, limits)?.package("fallback-api")?)?
        .build()?
        .resolve(native, limits)
        .and_then(|resolution| rawr_import::extract::from_resolution(resolution, native, limits))
}

fn environment(value: &str) -> Environment {
    dependency_environment(&fixture("freshness-api"), value)
}

fn dependency_environment(dependency: &Path, value: &str) -> Environment {
    Environment::capture()
        .and_then(|environment| environment.setting("CARGO_NET_OFFLINE", "true"))
        .and_then(|environment| {
            environment.setting(
                "FRESH_ABSOLUTE",
                dependency
                    .join("absolute.txt")
                    .to_str()
                    .expect("the fixture path is Unicode"),
            )
        })
        .and_then(|environment| environment.setting("FRESH_ENV", value))
        .and_then(|environment| environment.setting("FRESH_COMPILE_ENV", value))
        .and_then(|environment| environment.setting("FRESH_OPTIONAL_COMPILE_ENV", value))
        .expect("the declared build environment seals")
}

fn environment_without_freshness() -> Environment {
    Environment::capture()
        .and_then(|environment| environment.setting("CARGO_NET_OFFLINE", "true"))
        .and_then(|environment| {
            environment.setting(
                "FRESH_ABSOLUTE",
                fixture("freshness-api")
                    .join("absolute.txt")
                    .to_str()
                    .expect("the fixture path is Unicode"),
            )
        })
        .and_then(|environment| environment.setting("FRESH_COMPILE_ENV", "set"))
        .and_then(|environment| environment.removing("FRESH_OPTIONAL_COMPILE_ENV"))
        .and_then(|environment| environment.removing("FRESH_ENV"))
        .expect("the unset build environment seals")
}

fn fixture(name: &str) -> PathBuf {
    fs::canonicalize(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name),
    )
    .expect("the unchanged freshness fixture exists")
}

fn copy_directory(source: &Path, destination: &Path) {
    fs::create_dir(destination).expect("the copied fixture root is created");
    for entry in fs::read_dir(source).expect("the fixture directory is readable") {
        let entry = entry.expect("the fixture entry is readable");
        let destination = destination.join(entry.file_name());
        if entry
            .file_type()
            .expect("the fixture entry has a type")
            .is_dir()
        {
            copy_directory(&entry.path(), &destination);
        } else {
            fs::copy(entry.path(), destination).expect("the fixture file is copied");
        }
    }
}

fn only_named_file(root: &Path, name: &str) -> PathBuf {
    let mut files = Vec::new();
    collect_named_files(root, name, &mut files);
    assert_eq!(
        files.len(),
        1,
        "one `{name}` must exist under the build target"
    );
    files.pop().expect("the one generated file exists")
}

fn collect_named_files(path: &Path, name: &str, files: &mut Vec<PathBuf>) {
    let mut entries = fs::read_dir(path)
        .expect("the build target directory is readable")
        .collect::<std::result::Result<Vec<_>, _>>()
        .expect("the build target entries are readable");
    entries.sort_unstable_by_key(fs::DirEntry::file_name);
    for entry in entries {
        let path = entry.path();
        if entry
            .file_type()
            .expect("the build target entry has a type")
            .is_dir()
        {
            collect_named_files(&path, name, files);
        } else if entry.file_name() == name {
            files.push(path);
        }
    }
}

fn assert_canonical_equal(left: &[u8], right: &[u8]) {
    let left: serde_json::Value =
        serde_json::from_slice(left).expect("the first canonical graph is JSON");
    let right: serde_json::Value =
        serde_json::from_slice(right).expect("the second canonical graph is JSON");
    if let Some((path, left, right)) = first_difference("$", &left, &right) {
        panic!("canonical graphs differ at {path}: left={left} right={right}");
    }
}

fn first_difference(
    path: &str,
    left: &serde_json::Value,
    right: &serde_json::Value,
) -> Option<(String, String, String)> {
    match (left, right) {
        (serde_json::Value::Array(left), serde_json::Value::Array(right)) => {
            if left.len() != right.len() {
                return Some((
                    format!("{path}.length"),
                    left.len().to_string(),
                    right.len().to_string(),
                ));
            }
            left.iter()
                .zip(right)
                .enumerate()
                .find_map(|(index, (left, right))| {
                    first_difference(&format!("{path}[{index}]"), left, right)
                })
        }
        (serde_json::Value::Object(left), serde_json::Value::Object(right)) => {
            if path.starts_with("$.input.entries[") && left != right {
                return Some((
                    path.to_owned(),
                    serde_json::Value::Object(left.clone()).to_string(),
                    serde_json::Value::Object(right.clone()).to_string(),
                ));
            }
            if left.len() != right.len() {
                return Some((
                    format!("{path}.length"),
                    left.len().to_string(),
                    right.len().to_string(),
                ));
            }
            left.iter().find_map(|(key, left)| {
                right.get(key).map_or_else(
                    || {
                        Some((
                            format!("{path}.{key}"),
                            left.to_string(),
                            "<missing>".to_owned(),
                        ))
                    },
                    |right| first_difference(&format!("{path}.{key}"), left, right),
                )
            })
        }
        _ if left == right => None,
        _ => Some((path.to_owned(), left.to_string(), right.to_string())),
    }
}

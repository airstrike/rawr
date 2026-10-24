//! A project's runner must build the `rawr new` template's own units, so the
//! project has one compiled iced (D491). This compares Cargo's unit graphs of
//! the template and of a runner over it, without building: every unit the
//! template's `cargo build` compiles, its own crate aside, must be in the
//! runner's graph with the same features, profile and dependencies.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde_json::Value;

/// The pinned toolchain both builds use.
fn cargo(directory: &Path, arguments: &[&str]) -> Vec<u8> {
    let output = Command::new("cargo")
        .arg(format!("+{}", rawr_import::cargo::toolchain::CHANNEL))
        .args(arguments)
        .current_dir(directory)
        .env_remove("CARGO_TARGET_DIR")
        .env_remove("RUSTFLAGS")
        .output()
        .expect("cargo runs");
    assert!(
        output.status.success(),
        "cargo {arguments:?} in {} failed:\n{}",
        directory.display(),
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}

fn unit_graph(directory: &Path, extra: &[&str]) -> Value {
    let mut arguments = vec!["build", "--unit-graph", "-Z", "unstable-options"];
    arguments.extend_from_slice(extra);
    serde_json::from_slice(&cargo(directory, &arguments)).expect("the unit graph is JSON")
}

/// Each unit's identity: its own fields and, recursively, its
/// dependencies', as Cargo's `-C metadata` folds them in.
fn keys(graph: &Value) -> Vec<(String, String)> {
    let units = graph["units"].as_array().expect("units");
    fn local(unit: &Value) -> String {
        let mut profile = unit["profile"].clone();
        profile.as_object_mut().expect("profile").remove("name");
        serde_json::json!([
            unit["pkg_id"],
            unit["target"]["name"],
            unit["target"]["kind"],
            unit["mode"],
            unit["platform"],
            unit["features"],
            profile,
        ])
        .to_string()
    }
    fn key(index: usize, units: &[Value], memo: &mut BTreeMap<usize, String>) -> String {
        if let Some(key) = memo.get(&index) {
            return key.clone();
        }
        let mut dependencies = units[index]["dependencies"]
            .as_array()
            .expect("dependencies")
            .iter()
            .map(|dependency| key(dependency["index"].as_u64().unwrap() as usize, units, memo))
            .collect::<Vec<_>>();
        dependencies.sort();
        let key = format!("{}<{}>", local(&units[index]), dependencies.join(","));
        memo.insert(index, key.clone());
        key
    }
    let mut memo = BTreeMap::new();
    (0..units.len())
        .map(|index| (key(index, units, &mut memo), local(&units[index])))
        .collect()
}

#[test]
fn the_runner_builds_the_templates_own_units() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
    let root = fs::canonicalize(root).expect("the repository root");
    let scratch = root.join(format!("target/unit-graph-{}", std::process::id()));
    let _ = fs::remove_dir_all(&scratch);
    let project = scratch.join("app");
    let runner = scratch.join("runner");
    for directory in [project.join("src"), project.join(".cargo"), runner.join("src")] {
        fs::create_dir_all(directory).unwrap();
    }
    // The template, as `rawr new` writes it. Its own `[workspace]` keeps it
    // out of the repository's.
    fs::write(
        project.join("Cargo.toml"),
        format!("{}\n[workspace]\n", rawr_iced_cli::new::manifest("app")),
    )
    .unwrap();
    fs::write(project.join(".cargo/config.toml"), rawr_iced_cli::new::cargo_config()).unwrap();
    fs::write(project.join("rust-toolchain.toml"), rawr_iced_cli::new::toolchain()).unwrap();
    fs::write(project.join("src/main.rs"), "fn main() {}\n").unwrap();
    // The runner, as the importer generates it: Rawr's runtime crates, the
    // project's iced, every widget, and the rawr crate's own opt-level.
    let widgets = rawr_iced::features::Feature::ALL
        .map(|feature| format!("\"{}\"", feature.name()))
        .join(", ");
    let iced = rawr_iced_cli::new::ICED_FEATURES
        .map(|feature| format!("\"{feature}\""))
        .join(", ");
    let host = rawr_import::cargo::toolchain::HOST.trim();
    fs::write(
        runner.join("Cargo.toml"),
        format!(
            "[workspace]\nresolver = \"3\"\n\n\
             [package]\nname = \"rawr-generated-runner\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n\
             [dependencies]\n\
             __rawr_internal = {{ package = \"rawr\", path = {root:?}, default-features = false, features = [\"rust-import\", \"std\"] }}\n\
             __rawr_profile_host = {{ package = \"rawr-iced\", path = {iced_path:?}, default-features = false, features = [{widgets}] }}\n\
             iced = {{ git = \"https://github.com/iced-rs/iced\", rev = \"{revision}\", default-features = true, features = [{iced}] }}\n\n\
             [profile.dev]\nincremental = false\n\n\
             [profile.dev.package]\nrawr-generated-runner = {{ incremental = true }}\n\n\
             [profile.dev.package.rawr]\ndebug = 0\nopt-level = 3\nstrip = false\n",
            root = root.display().to_string(),
            iced_path = root.join("iced").display().to_string(),
            revision = rawr_iced::native::ICED_REVISION,
        ),
    )
    .unwrap();
    fs::write(runner.join("src/main.rs"), "fn main() {}\n").unwrap();
    fs::write(runner.join("rust-toolchain.toml"), rawr_iced_cli::new::toolchain()).unwrap();

    cargo(&project, &["generate-lockfile"]);
    // The runner's resolution starts from the project's lock.
    fs::copy(project.join("Cargo.lock"), runner.join("Cargo.lock")).unwrap();
    let template = keys(&unit_graph(&project, &[]));
    let built = keys(&unit_graph(&runner, &["--target", host]));
    fs::remove_dir_all(&scratch).unwrap();

    let shared = built.iter().map(|(key, _)| key.as_str()).collect::<BTreeSet<_>>();
    let runner_local = built.iter().map(|(_, local)| local.as_str()).collect::<BTreeSet<_>>();
    let missing = template
        .iter()
        .filter(|(key, local)| !shared.contains(key.as_str()) && !local.contains("\"app\""))
        .map(|(_, local)| local.as_str())
        .collect::<Vec<_>>();
    // A unit whose own fields differ is a cause; the rest differ only through
    // a dependency.
    let causes = missing
        .iter()
        .filter(|local| !runner_local.contains(*local))
        .collect::<Vec<_>>();
    assert!(
        missing.is_empty(),
        "{} of the template's units are not the runner's; these differ in their own \
         features or profile:\n{}",
        missing.len(),
        causes.iter().map(|cause| format!("  {cause}")).collect::<Vec<_>>().join("\n")
    );
}

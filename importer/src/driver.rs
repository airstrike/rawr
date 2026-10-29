//! Rawr-owned external-metadata probe for exact Cargo `.rmeta` artifacts.

#![feature(rustc_private)]

extern crate rustc_data_structures;
extern crate rustc_driver;
extern crate rustc_hir;
extern crate rustc_infer;
extern crate rustc_interface;
extern crate rustc_log;
extern crate rustc_middle;
extern crate rustc_session;
extern crate rustc_span;
extern crate rustc_trait_selection;

use std::collections::BTreeSet;
use std::env;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde::Deserialize;
use sha2::{Digest as _, Sha256};

#[path = "driver/compiler.rs"]
mod compiler;

#[path = "extract/protocol/attempt.rs"]
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "shared with the library, which decodes; this binary only encodes"
    )
)]
mod attempt_protocol;

const PLAN_SCHEMA: u32 = 8;
// One number in four places: `driver/compiler.rs`, `cargo/toolchain.rs`
// (`SUPPORTED_PROBE_PROTOCOL`) and `extract/protocol.rs` (`SCHEMA`) move with it.
const PROTOCOL_SCHEMA: u32 = 31;
const COMPILER_COMMIT: &str = "375b1431b7d89d1c2e2bc168c011848ae12b7d14";
const MAX_BYTES: usize = 256 * 1024 * 1024;
const MAX_ITEMS: usize = 2_500_000;
const CANONICAL_ITEMS: usize = 19_000_000;
const CANONICAL_BYTES: usize = 1_207_959_552;
const MAX_STRING: usize = 1024 * 1024;
// The host's PATH_MAX: 1024 on macOS, 4096 on Linux.
const CANONICAL_PATH_BYTES: usize = libc::PATH_MAX as usize;
const RUST_MIN_STACK_VALUE: &str = "67108864";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    schema: u32,
    protocol_schema: u32,
    canonical_items: usize,
    canonical_bytes: usize,
    compiler_commit: String,
    host: String,
    selected_target: String,
    entries: Vec<Entry>,
    dependency_directories: Vec<String>,
    /// Paths, root alias first, through wholesale crate re-exports.
    #[serde(default)]
    demand: Vec<Vec<String>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    package: Package,
    root: Root,
    unit_key: String,
    dependencies: Vec<String>,
    target: String,
    target_kind: String,
    target_kinds: Vec<String>,
    crate_types: Vec<String>,
    crate_name: String,
    edition: String,
    profile: String,
    features: Vec<String>,
    invocation_nonce: String,
    unit_correlation: String,
    artifact: Artifact,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Artifact {
    crate_name: String,
    path: String,
    length: u64,
    digest: String,
    profile: String,
    fresh: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Package {
    cargo_id: String,
    name: String,
    version: String,
    source: String,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Root {
    Selected { aliases: Vec<String> },
}

fn main() -> ExitCode {
    match execute() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("rawr-import-driver: {error}");
            ExitCode::FAILURE
        }
    }
}

fn execute() -> Result<ExitCode, String> {
    let mut budget = compiler::Budget::standard();
    let arguments = bounded_arguments(&mut budget)?;
    if arguments.len() == 2 && arguments[1] == "--version" {
        println!("rawr-import-driver {PLAN_SCHEMA} {PROTOCOL_SCHEMA} {COMPILER_COMMIT}");
        return Ok(ExitCode::SUCCESS);
    }
    if !(6..=7).contains(&arguments.len()) {
        return Err(
            "usage: rawr-import-driver (--probe|--diagnostic-probe) RUSTC PLAN PROTOCOL_DIRECTORY PROBE_SOURCE [REUSE_DIRECTORY]".to_owned(),
        );
    }
    let diagnose_recursion = match arguments[1].as_str() {
        "--probe" => false,
        "--diagnostic-probe" => true,
        _ => {
            return Err(
                "usage: rawr-import-driver (--probe|--diagnostic-probe) RUSTC PLAN PROTOCOL_DIRECTORY PROBE_SOURCE [REUSE_DIRECTORY]".to_owned(),
            );
        }
    };
    validate_rustc_stack_authority(env::var_os("RUST_MIN_STACK").as_deref())?;
    let rustc = canonical_file(Path::new(&arguments[2]), "pinned rustc", &mut budget)?;
    let plan_path = canonical_file(Path::new(&arguments[3]), "metadata probe plan", &mut budget)?;
    let protocol_directory = canonical_directory(
        Path::new(&arguments[4]),
        "metadata probe protocol directory",
        &mut budget,
    )?;
    let probe_source = canonical_file(
        Path::new(&arguments[5]),
        "metadata probe source",
        &mut budget,
    )?;
    // Where unchanged crates' extraction bodies are kept.
    let reuse = arguments
        .get(6)
        .map(|directory| {
            canonical_directory(
                Path::new(directory),
                "extraction reuse directory",
                &mut budget,
            )
        })
        .transpose()?;
    let plan = read_plan(&plan_path, &mut budget)?;
    validate_plan(&plan, &mut budget)?;

    let sysroot = rustc
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| "pinned rustc has no sysroot parent".to_owned())?;
    let sysroot = canonical_directory(sysroot, "pinned rustc sysroot", &mut budget)?;
    let mut compiler_arguments = Vec::new();
    let argument_count = 10_usize.saturating_add(
        plan.entries
            .len()
            .saturating_mul(2)
            .saturating_add(plan.dependency_directories.len()),
    );
    budget.items(argument_count)?;
    reserve(&mut compiler_arguments, argument_count)?;
    compiler_arguments.push(budget.owned(path_text(&rustc)?)?);
    compiler_arguments.push(budget.owned("--crate-name=rawr_metadata_probe")?);
    compiler_arguments.push(budget.owned("--edition=2024")?);
    compiler_arguments.push(budget.owned("--crate-type=rlib")?);
    compiler_arguments.push(budget.formatted(format_args!("--target={}", plan.selected_target))?);
    compiler_arguments.push(budget.formatted(format_args!("--sysroot={}", path_text(&sysroot)?))?);
    compiler_arguments.push(budget.owned("--emit=metadata")?);
    compiler_arguments.push(budget.owned("-Zunstable-options")?);
    compiler_arguments.push(budget.owned("--cap-lints=allow")?);
    for directory in &plan.dependency_directories {
        compiler_arguments.push(budget.formatted(format_args!("-Ldependency={directory}"))?);
    }

    let mut invocations = Vec::new();
    budget.items(plan.entries.len())?;
    reserve(&mut invocations, plan.entries.len())?;
    for (index, entry) in plan.entries.into_iter().enumerate() {
        let alias = budget.formatted(format_args!("rawr_selected_{index}"))?;
        compiler_arguments.push(budget.owned("--extern")?);
        compiler_arguments.push(budget.formatted(format_args!(
            "force,noprelude:{alias}={}",
            entry.artifact.path
        ))?);
        let Root::Selected { aliases } = entry.root;
        let mut demand = Vec::new();
        for path in plan.demand.iter().filter(|path| aliases.contains(&path[0])) {
            budget.items(path.len())?;
            demand.push(path.clone());
        }
        invocations.push(compiler::Invocation {
            package: compiler::Package {
                cargo_id: entry.package.cargo_id,
                name: entry.package.name,
                version: entry.package.version,
                source: entry.package.source,
            },
            root: compiler::Root::Selected { aliases },
            demand,
            unit_key: entry.unit_key,
            dependencies: entry.dependencies,
            target: entry.target,
            target_kind: entry.target_kind,
            target_kinds: entry.target_kinds,
            crate_types: entry.crate_types,
            crate_name: entry.crate_name,
            edition: entry.edition,
            profile: entry.profile,
            host: budget.owned(&plan.host)?,
            selected_target: budget.owned(&plan.selected_target)?,
            features: entry.features,
            invocation_nonce: entry.invocation_nonce,
            unit_correlation: entry.unit_correlation,
            compiler_commit: budget.owned(&plan.compiler_commit)?,
            protocol_directory: {
                budget.items(1)?;
                PathBuf::from(budget.owned(path_text(&protocol_directory)?)?)
            },
            artifact: compiler::Artifact {
                path: entry.artifact.path,
                length: entry.artifact.length,
                digest: entry.artifact.digest,
                profile: entry.artifact.profile,
                fresh: entry.artifact.fresh,
                probe_alias: alias,
            },
        });
    }
    compiler_arguments.push(budget.owned(path_text(&probe_source)?)?);
    compiler::run(
        compiler_arguments,
        invocations,
        budget,
        diagnose_recursion,
        reuse,
    )
}

fn validate_rustc_stack_authority(value: Option<&OsStr>) -> Result<(), String> {
    if value == Some(OsStr::new(RUST_MIN_STACK_VALUE)) {
        Ok(())
    } else {
        Err(format!(
            "RUST_MIN_STACK must be exactly {RUST_MIN_STACK_VALUE} for metadata probes"
        ))
    }
}

fn bounded_arguments(budget: &mut compiler::Budget) -> Result<Vec<String>, String> {
    let mut arguments = Vec::new();
    let mut bytes = 0_usize;
    for argument in env::args_os() {
        if arguments.len() >= MAX_ITEMS {
            return Err("metadata probe argument count limit exceeded".to_owned());
        }
        let argument = argument
            .into_string()
            .map_err(|_| "metadata probe argument is not UTF-8".to_owned())?;
        checked_string(&argument)?;
        bytes = bytes.saturating_add(argument.len());
        if bytes > MAX_BYTES {
            return Err("metadata probe argument byte limit exceeded".to_owned());
        }
        capture_argument(&mut arguments, argument, budget)?;
    }
    Ok(arguments)
}

fn read_plan(path: &Path, budget: &mut compiler::Budget) -> Result<Plan, String> {
    let bytes = read_bounded_file(path, "metadata probe plan", budget)?;
    decode_plan(&bytes, budget)
}

fn decode_plan(bytes: &[u8], budget: &mut compiler::Budget) -> Result<Plan, String> {
    let usage = preflight_json(bytes)?;
    budget.items(usage.items)?;
    budget.bytes(usage.bytes)?;
    serde_json::from_slice(bytes).map_err(|error| format!("invalid metadata probe plan: {error}"))
}

fn capture_argument(
    arguments: &mut Vec<String>,
    argument: String,
    budget: &mut compiler::Budget,
) -> Result<(), String> {
    budget.items(1)?;
    budget.string(&argument)?;
    reserve(arguments, 1)?;
    arguments.push(argument);
    Ok(())
}

fn read_bounded_file(
    path: &Path,
    label: &str,
    budget: &mut compiler::Budget,
) -> Result<Vec<u8>, String> {
    read_bounded_file_with_limit(path, label, MAX_BYTES, budget)
}

fn read_bounded_file_with_limit(
    path: &Path,
    label: &str,
    limit: usize,
    budget: &mut compiler::Budget,
) -> Result<Vec<u8>, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("failed to inspect {label}: {error}"))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(format!("{label} is not a regular non-symlink file"));
    }
    let length = usize::try_from(metadata.len())
        .map_err(|_| format!("{label} length is not addressable"))?;
    if length > limit {
        return Err(format!("{label} byte limit exceeded"));
    }
    budget.items(1)?;
    budget.bytes(length)?;
    let mut bytes = Vec::new();
    reserve(&mut bytes, length)?;
    bytes.resize(length, 0);
    let mut file = File::open(path).map_err(|error| format!("failed to open {label}: {error}"))?;
    read_expected_length(&mut file, bytes, label)
}

fn read_expected_length(
    reader: &mut impl std::io::Read,
    mut bytes: Vec<u8>,
    label: &str,
) -> Result<Vec<u8>, String> {
    if let Err(error) = reader.read_exact(&mut bytes) {
        if error.kind() == std::io::ErrorKind::UnexpectedEof {
            return Err(format!("{label} changed while it was read"));
        }
        return Err(format!("failed to read {label}: {error}"));
    }
    let mut extra = [0_u8; 1];
    loop {
        match reader.read(&mut extra) {
            Ok(0) => return Ok(bytes),
            Ok(_) => return Err(format!("{label} changed while it was read")),
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => return Err(format!("failed to probe {label} length: {error}")),
        }
    }
}

fn artifact_matches(path: &Path, artifact: &Artifact) -> Result<bool, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| format!("failed to inspect selected rmeta: {error}"))?;
    if !metadata.is_file()
        || metadata.file_type().is_symlink()
        || metadata.len() != artifact.length
        || metadata.len() > MAX_BYTES as u64
    {
        return Ok(false);
    }
    let mut file =
        File::open(path).map_err(|error| format!("failed to open selected rmeta: {error}"))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut length = 0_u64;
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|error| format!("failed to hash selected rmeta: {error}"))?;
        if count == 0 {
            break;
        }
        length = length
            .checked_add(count as u64)
            .ok_or_else(|| "selected rmeta length overflow".to_owned())?;
        if length > MAX_BYTES as u64 {
            return Ok(false);
        }
        digest.update(&buffer[..count]);
    }
    Ok(length == artifact.length
        && exact_digest(&artifact.digest).is_some_and(|expected| digest.finalize()[..] == expected))
}

struct JsonUsage {
    items: usize,
    bytes: usize,
}

fn preflight_json(bytes: &[u8]) -> Result<JsonUsage, String> {
    let mut depth = 0_usize;
    let mut items = 0_usize;
    let mut string_start = None;
    let mut escaped = false;
    let mut strings = 0_usize;
    for (index, byte) in bytes.iter().copied().enumerate() {
        if let Some(start) = string_start {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                let length = index.saturating_sub(start);
                if length > MAX_STRING {
                    return Err("metadata probe JSON string limit exceeded".to_owned());
                }
                strings = strings
                    .checked_add(length)
                    .ok_or_else(|| "metadata probe JSON string bytes overflow".to_owned())?;
                if strings > MAX_BYTES {
                    return Err("metadata probe JSON aggregate string limit exceeded".to_owned());
                }
                string_start = None;
                items = items.saturating_add(1);
            }
            continue;
        }
        match byte {
            b'"' => string_start = Some(index.saturating_add(1)),
            b'{' | b'[' => {
                depth = depth.saturating_add(1);
                items = items.saturating_add(1);
                if depth > 256 {
                    return Err("metadata probe JSON depth limit exceeded".to_owned());
                }
            }
            b'}' | b']' => depth = depth.saturating_sub(1),
            b',' => items = items.saturating_add(1),
            _ => {}
        }
        if items > MAX_ITEMS {
            return Err("metadata probe JSON item limit exceeded".to_owned());
        }
    }
    if string_start.is_some() || depth != 0 {
        return Err("metadata probe JSON is structurally incomplete".to_owned());
    }
    Ok(JsonUsage {
        items,
        bytes: strings,
    })
}

fn validate_plan(plan: &Plan, budget: &mut compiler::Budget) -> Result<(), String> {
    if plan.schema != PLAN_SCHEMA
        || plan.protocol_schema != PROTOCOL_SCHEMA
        || plan.canonical_items != CANONICAL_ITEMS
        || plan.canonical_bytes != CANONICAL_BYTES
        || plan.compiler_commit != COMPILER_COMMIT
        || plan.host != include_str!(concat!(env!("OUT_DIR"), "/toolchain-host"))
        || plan.selected_target != plan.host
        || plan.entries.is_empty()
        || plan.entries.len() > MAX_ITEMS
        || plan.dependency_directories.len() > MAX_ITEMS
        || plan.demand.len() > MAX_ITEMS
        || plan
            .demand
            .iter()
            .any(|path| path.len() < 2 || path.iter().any(String::is_empty))
    {
        return Err("metadata probe plan authority is invalid".to_owned());
    }
    let mut units = BTreeSet::new();
    let mut nonces = BTreeSet::new();
    let mut artifacts = BTreeSet::new();
    let mut dependency_directories = BTreeSet::new();
    for directory in &plan.dependency_directories {
        let path = canonical_directory(Path::new(directory), "probe dependency directory", budget)?;
        if !insert_unique(&mut dependency_directories, path, budget)? {
            return Err("duplicate probe dependency directory".to_owned());
        }
    }
    for entry in &plan.entries {
        let path = canonical_file(
            Path::new(&entry.artifact.path),
            "selected rmeta artifact",
            budget,
        )?;
        if !artifact_matches(&path, &entry.artifact)? {
            return Err("selected rmeta disagrees with Cargo artifact evidence".to_owned());
        }
        if path.extension().is_none_or(|value| value != "rmeta")
            || entry.target_kinds.is_empty()
            || entry.crate_types.is_empty()
            || entry.profile.is_empty()
            || entry.artifact.crate_name != entry.crate_name
            || entry.artifact.profile.is_empty()
        {
            return Err("metadata probe entry is invalid or ambiguous".to_owned());
        }
        if !insert_unique(&mut units, entry.unit_correlation.as_str(), budget)?
            || !insert_unique(&mut nonces, entry.invocation_nonce.as_str(), budget)?
            || !insert_unique(&mut artifacts, path, budget)?
        {
            return Err("metadata probe entry is invalid or ambiguous".to_owned());
        }
    }
    if artifacts.iter().any(|path| {
        path.parent()
            .is_none_or(|parent| !dependency_directories.contains(parent))
    }) {
        return Err("probe dependency directories omit an exact artifact parent".to_owned());
    }
    Ok(())
}

fn insert_unique<T: Ord>(
    values: &mut BTreeSet<T>,
    value: T,
    budget: &mut compiler::Budget,
) -> Result<bool, String> {
    if values.contains(&value) {
        return Ok(false);
    }
    budget.items(1)?;
    values.insert(value);
    Ok(true)
}

fn canonical_file(
    path: &Path,
    label: &str,
    budget: &mut compiler::Budget,
) -> Result<PathBuf, String> {
    budget.path_capacity(CANONICAL_PATH_BYTES)?;
    let canonical = fs::canonicalize(path)
        .map_err(|error| format!("failed to canonicalize {label}: {error}"))?;
    budget.validate_path(&canonical, CANONICAL_PATH_BYTES)?;
    if !canonical.is_file() {
        return Err(format!("{label} is not a file"));
    }
    Ok(canonical)
}

fn canonical_directory(
    path: &Path,
    label: &str,
    budget: &mut compiler::Budget,
) -> Result<PathBuf, String> {
    budget.path_capacity(CANONICAL_PATH_BYTES)?;
    let canonical = fs::canonicalize(path)
        .map_err(|error| format!("failed to canonicalize {label}: {error}"))?;
    budget.validate_path(&canonical, CANONICAL_PATH_BYTES)?;
    if !canonical.is_dir() {
        return Err(format!("{label} is not a directory"));
    }
    Ok(canonical)
}

fn path_text(path: &Path) -> Result<&str, String> {
    path.to_str()
        .ok_or_else(|| format!("path is not UTF-8: {}", path.display()))
}

fn checked_string(value: &str) -> Result<(), String> {
    if value.len() > MAX_STRING {
        Err("metadata probe string limit exceeded".to_owned())
    } else {
        Ok(())
    }
}

fn reserve<T>(values: &mut Vec<T>, additional: usize) -> Result<(), String> {
    values
        .try_reserve_exact(additional)
        .map_err(|error| format!("metadata probe bounded allocation failed: {error}"))
}

fn exact_digest(value: &str) -> Option<[u8; 32]> {
    if value.len() != 64 {
        return None;
    }
    let mut digest = [0_u8; 32];
    for (output, pair) in digest
        .iter_mut()
        .zip(value.as_bytes().as_chunks::<2>().0.iter())
    {
        let high = hex_digit(pair[0])?;
        let low = hex_digit(pair[1])?;
        *output = high.checked_mul(16)?.checked_add(low)?;
    }
    Some(digest)
}

const fn hex_digit(value: u8) -> Option<u8> {
    match value {
        b'0'..=b'9' => Some(value - b'0'),
        b'a'..=b'f' => Some(value - b'a' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use serde_json::json;

    use super::{
        canonical_file, capture_argument, compiler, decode_plan, read_bounded_file_with_limit,
        read_expected_length, validate_plan, validate_rustc_stack_authority, CANONICAL_BYTES,
        CANONICAL_ITEMS, CANONICAL_PATH_BYTES, PLAN_SCHEMA, PROTOCOL_SCHEMA, RUST_MIN_STACK_VALUE,
    };

    fn plan_bytes(aliases: &[&str]) -> Vec<u8> {
        serde_json::to_vec(&json!({
            "schema": PLAN_SCHEMA,
            "protocol_schema": PROTOCOL_SCHEMA,
            "canonical_items": CANONICAL_ITEMS,
            "canonical_bytes": CANONICAL_BYTES,
            "compiler_commit": "commit",
            "host": "host",
            "selected_target": "target",
            "entries": [{
                "package": {"cargo_id": "id", "name": "name", "version": "version", "source": "source"},
                "root": {"kind": "selected", "aliases": aliases},
                "unit_key": "unit",
                "dependencies": ["dependency"],
                "target": "src/lib.rs",
                "target_kind": "lib",
                "target_kinds": ["lib"],
                "crate_types": ["rlib"],
                "crate_name": "fixture",
                "edition": "2024",
                "profile": "dev",
                "features": ["feature"],
                "invocation_nonce": "nonce",
                "unit_correlation": "correlation",
                "artifact": {
                    "crate_name": "fixture",
                    "path": "artifact.rmeta",
                    "length": 1,
                    "digest": "digest",
                    "profile": "dev",
                    "fresh": false
                }
            }],
            "dependency_directories": ["dependency-directory"]
        }))
        .expect("the fixture plan encodes")
    }

    #[test]
    fn plan_decode_budget_has_an_exact_byte_boundary() {
        let bytes = plan_bytes(&["root"]);
        let mut measured = compiler::Budget::with_limits(usize::MAX, usize::MAX);
        decode_plan(&bytes, &mut measured).expect("the fixture plan is measurable");
        let (bytes, _, _) = measured.usage();

        let mut exact = compiler::Budget::with_limits(bytes, usize::MAX);
        decode_plan(&plan_bytes(&["root"]), &mut exact)
            .expect("the exact allocation boundary accepts");
        let mut short_bytes = compiler::Budget::with_limits(bytes - 1, usize::MAX);
        assert!(decode_plan(&plan_bytes(&["root"]), &mut short_bytes).is_err());
    }

    #[test]
    fn prior_plan_and_protocol_authority_reject_before_filesystem_validation() {
        assert_eq!(CANONICAL_ITEMS, 19_000_000);
        assert_eq!(CANONICAL_BYTES, 1_207_959_552);
        let mut budget = compiler::Budget::with_limits(usize::MAX, usize::MAX);
        let mut old_plan = decode_plan(&plan_bytes(&["root"]), &mut budget)
            .expect("the current fixture plan decodes");
        old_plan.schema = PLAN_SCHEMA - 1;
        assert_eq!(
            validate_plan(&old_plan, &mut budget),
            Err("metadata probe plan authority is invalid".to_owned())
        );

        let mut budget = compiler::Budget::with_limits(usize::MAX, usize::MAX);
        let mut old_protocol = decode_plan(&plan_bytes(&["root"]), &mut budget)
            .expect("the current fixture plan decodes");
        old_protocol.protocol_schema = PROTOCOL_SCHEMA - 1;
        assert_eq!(
            validate_plan(&old_protocol, &mut budget),
            Err("metadata probe plan authority is invalid".to_owned())
        );

        let mut budget = compiler::Budget::with_limits(usize::MAX, usize::MAX);
        let mut old_canonical = decode_plan(&plan_bytes(&["root"]), &mut budget)
            .expect("the current fixture plan decodes");
        old_canonical.canonical_items = CANONICAL_ITEMS - 1;
        assert_eq!(
            validate_plan(&old_canonical, &mut budget),
            Err("metadata probe plan authority is invalid".to_owned())
        );

        let mut budget = compiler::Budget::with_limits(usize::MAX, usize::MAX);
        let mut old_canonical_bytes = decode_plan(&plan_bytes(&["root"]), &mut budget)
            .expect("the current fixture plan decodes");
        old_canonical_bytes.canonical_bytes = CANONICAL_BYTES - 1;
        assert_eq!(
            validate_plan(&old_canonical_bytes, &mut budget),
            Err("metadata probe plan authority is invalid".to_owned())
        );
    }

    #[test]
    fn argument_path_and_validation_nodes_are_prospectively_bounded() {
        let mut measured = compiler::Budget::with_limits(usize::MAX, usize::MAX);
        let mut arguments = Vec::new();
        capture_argument(&mut arguments, "argument".to_owned(), &mut measured)
            .expect("the argument is measurable");
        let (argument_bytes, _, _) = measured.usage();
        let mut exact = compiler::Budget::with_limits(argument_bytes, usize::MAX);
        capture_argument(&mut Vec::new(), "argument".to_owned(), &mut exact)
            .expect("the exact argument boundary accepts");
        let mut short = compiler::Budget::with_limits(argument_bytes - 1, usize::MAX);
        assert!(capture_argument(&mut Vec::new(), "argument".to_owned(), &mut short).is_err());

        let executable = std::env::current_exe().expect("the test executable has a path");
        let mut exact_path = compiler::Budget::with_limits(CANONICAL_PATH_BYTES, usize::MAX);
        canonical_file(&executable, "test executable", &mut exact_path)
            .expect("the exact prospective path boundary accepts");
        let mut short_path = compiler::Budget::with_limits(CANONICAL_PATH_BYTES - 1, usize::MAX);
        assert!(canonical_file(&executable, "test executable", &mut short_path).is_err());
    }

    #[test]
    fn file_read_allocation_is_exact_and_drift_uses_a_stack_probe() {
        let path =
            std::env::temp_dir().join(format!("rawr-import-driver-read-{}", std::process::id()));
        std::fs::write(&path, b"four").expect("the bounded-read fixture is written");

        let mut exact = compiler::Budget::with_limits(4, usize::MAX);
        let bytes = read_bounded_file_with_limit(&path, "test plan", 4, &mut exact)
            .expect("the exact file-read allocation accepts");
        assert_eq!(bytes, b"four");
        let mut byte_short = compiler::Budget::with_limits(3, usize::MAX);
        assert!(read_bounded_file_with_limit(&path, "test plan", 4, &mut byte_short).is_err());

        std::fs::remove_file(&path).expect("the bounded-read fixture is retired");

        let short = read_expected_length(&mut Cursor::new(b"ab"), vec![0_u8; 3], "short fixture");
        assert!(short.is_err_and(|error| error.contains("changed while it was read")));
        let grown = read_expected_length(&mut Cursor::new(b"abcd"), vec![0_u8; 3], "grown fixture");
        assert!(grown.is_err_and(|error| error.contains("changed while it was read")));
    }

    #[test]
    fn rustc_probe_requires_the_exact_attested_inner_stack_authority() {
        validate_rustc_stack_authority(Some(std::ffi::OsStr::new(RUST_MIN_STACK_VALUE)))
            .expect("the exact sealed stack authority is accepted");
        assert!(validate_rustc_stack_authority(None).is_err());
        assert!(validate_rustc_stack_authority(Some(std::ffi::OsStr::new("16777216"))).is_err());
    }
}

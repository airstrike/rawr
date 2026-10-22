//! Versioned semantic environment freshness for one sealed Cargo request.

use std::collections::BTreeMap;

use sha2::{Digest as _, Sha256};

use super::Environment;
use crate::cargo::{self, toolchain};
use crate::process::parallelism;
use crate::{Error, Result};

const SCHEMA: u32 = 10;

pub(super) const fn schema() -> u32 {
    SCHEMA
}

// Audited against the pinned Cargo environment/config keys that can alter
// compiler inputs or emitted artifacts. CARGO_HOME and HOME also participate in
// config-candidate discovery; retaining both here makes their wider tool/path
// authority explicit instead of relying on that derived-path overlap.
// Authentication, networking, terminal, logging, and unrelated operational
// cache/concurrency controls are intentionally absent.
const STATIC_KEYS: [&str; 14] = [
    "CARGO_BUILD_DEP_INFO_BASEDIR",
    "CARGO_BUILD_RUSTDOCFLAGS",
    "CARGO_BUILD_RUSTFLAGS",
    "CARGO_ENCODED_RUSTDOCFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
    "CARGO_HOME",
    "DEVELOPER_DIR",
    "HOME",
    "MACOSX_DEPLOYMENT_TARGET",
    "PATH",
    "RUSTC_BOOTSTRAP",
    "RUSTDOCFLAGS",
    "RUSTFLAGS",
    "SDKROOT",
];
const TARGET_SUFFIXES: [&str; 3] = ["LINKER", "RUSTDOCFLAGS", "RUSTFLAGS"];
const PROFILE_SUFFIXES: [&str; 10] = [
    "CODEGEN_UNITS",
    "DEBUG",
    "DEBUG_ASSERTIONS",
    "LTO",
    "OPT_LEVEL",
    "OVERFLOW_CHECKS",
    "PANIC",
    "RPATH",
    "SPLIT_DEBUGINFO",
    "STRIP",
];
const BUILD_OVERRIDE_SUFFIXES: [&str; 7] = [
    "CODEGEN_UNITS",
    "DEBUG",
    "DEBUG_ASSERTIONS",
    "OPT_LEVEL",
    "OVERFLOW_CHECKS",
    "SPLIT_DEBUGINFO",
    "STRIP",
];

/// One opaque, versioned semantic projection born from a sealed environment.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Projection {
    target: Box<str>,
    profile: Box<str>,
    entries: Box<[Entry]>,
}

#[derive(Clone, Debug, Eq, Ord, PartialEq, PartialOrd)]
struct Entry {
    key: Box<str>,
    value: Option<Box<str>>,
}

impl Projection {
    pub(super) fn universe_digest(&self) -> cargo::Digest {
        let mut digest = Sha256::new();
        digest.update(b"rawr.import.cargo.config-projection/v1");
        digest.update(SCHEMA.to_le_bytes());
        for value in [&self.target, &self.profile] {
            digest.update((value.len() as u64).to_le_bytes());
            digest.update(value.as_bytes());
        }
        let entries = self
            .entries
            .iter()
            .filter(|entry| universe_entry(&entry.key));
        for entry in entries {
            digest.update((entry.key.len() as u64).to_le_bytes());
            digest.update(entry.key.as_bytes());
            match &entry.value {
                Some(value) => {
                    digest.update([1]);
                    digest.update((value.len() as u64).to_le_bytes());
                    digest.update(value.as_bytes());
                }
                None => digest.update([0]),
            }
        }
        cargo::Digest(digest.finalize().into())
    }

    pub(crate) fn derive(
        environment: &Environment,
        target: &str,
        profile: &str,
        limits: &cargo::Limits,
    ) -> Result<Self> {
        let parallelism = parallelism::Proof::discover()?;
        Self::derive_governed(environment, target, profile, parallelism, limits)
    }

    pub(crate) fn derive_governed(
        environment: &Environment,
        target: &str,
        profile: &str,
        parallelism: parallelism::Proof,
        limits: &cargo::Limits,
    ) -> Result<Self> {
        let mut allocation = cargo::RequestAllocation::new(limits);
        Self::derive_governed_budgeted(
            environment,
            target,
            profile,
            parallelism,
            limits,
            &mut allocation,
        )
    }

    pub(super) fn derive_governed_budgeted(
        environment: &Environment,
        target: &str,
        profile: &str,
        parallelism: parallelism::Proof,
        limits: &cargo::Limits,
        allocation: &mut cargo::RequestAllocation,
    ) -> Result<Self> {
        bounded("environment projection target", target, limits, allocation);
        bounded(
            "environment projection profile",
            profile,
            limits,
            allocation,
        );
        allocation.string(target)?;
        allocation.string(profile)?;
        let mut values = BTreeMap::<String, Option<Box<str>>>::new();
        for key in STATIC_KEYS {
            insert_environment(&mut values, environment, key, limits, allocation)?;
        }
        let mut platforms = [toolchain::HOST, target];
        platforms.sort_unstable();
        let platform_count = if platforms[0] == platforms[1] { 1 } else { 2 };
        for platform in platforms.into_iter().take(platform_count) {
            let platform = component(platform, limits, allocation)?;
            for suffix in TARGET_SUFFIXES {
                let key = derived_key("CARGO_TARGET_", &platform, "_", suffix, limits, allocation)?;
                insert_environment(&mut values, environment, &key, limits, allocation)?;
            }
        }
        let profile_component = component(profile, limits, allocation)?;
        let profile_prefix = derived_key(
            "CARGO_PROFILE_",
            &profile_component,
            "_",
            "",
            limits,
            allocation,
        )?;
        for (key, _) in environment.entries() {
            if !key.starts_with("CARGO_PROFILE_") {
                continue;
            }
            let allowed = key.strip_prefix(&profile_prefix).is_some_and(|suffix| {
                PROFILE_SUFFIXES.contains(&suffix)
                    || suffix
                        .strip_prefix("BUILD_OVERRIDE_")
                        .is_some_and(|suffix| BUILD_OVERRIDE_SUFFIXES.contains(&suffix))
            });
            if !allowed {
                return Err(Error::Request {
                    field: "Cargo profile environment authority",
                    reason: format!("unsupported Cargo profile environment variable `{key}`")
                        .into_boxed_str(),
                });
            }
        }
        for suffix in PROFILE_SUFFIXES {
            let key = derived_key(
                "CARGO_PROFILE_",
                &profile_component,
                "_",
                suffix,
                limits,
                allocation,
            )?;
            insert_environment(&mut values, environment, &key, limits, allocation)?;
        }
        for suffix in BUILD_OVERRIDE_SUFFIXES {
            let key = derived_key(
                "CARGO_PROFILE_",
                &profile_component,
                "_BUILD_OVERRIDE_",
                suffix,
                limits,
                allocation,
            )?;
            insert_environment(&mut values, environment, &key, limits, allocation)?;
        }

        insert_derived(&mut values, "CARGO_INCREMENTAL", "0", limits, allocation)?;
        insert_derived(
            &mut values,
            "rawr.context.cargo_commit",
            toolchain::supported_cargo_commit(),
            limits,
            allocation,
        )?;
        insert_derived(
            &mut values,
            "rawr.context.rustc_commit",
            toolchain::supported_rustc_commit(),
            limits,
            allocation,
        )?;
        insert_derived(
            &mut values,
            "rawr.context.host",
            toolchain::HOST,
            limits,
            allocation,
        )?;
        insert_derived(
            &mut values,
            "rawr.context.target",
            target,
            limits,
            allocation,
        )?;
        insert_derived(
            &mut values,
            "rawr.context.profile",
            profile,
            limits,
            allocation,
        )?;
        for (key, value) in [
            (
                "rawr.context.parallelism_schema",
                allocation.formatted(format_args!("{}", parallelism.schema()))?,
            ),
            (
                "rawr.context.jobs",
                allocation.formatted(format_args!("{}", parallelism.jobs().get()))?,
            ),
            (
                "rawr.context.canonical_allocation_items",
                allocation.formatted(format_args!("{}", limits.canonical_items))?,
            ),
            (
                "rawr.context.canonical_allocation_bytes",
                allocation.formatted(format_args!("{}", limits.canonical_bytes))?,
            ),
            (
                "rawr.context.input_proof_bytes",
                allocation.formatted(format_args!("{}", limits.input_proof_bytes))?,
            ),
            (
                "rawr.context.retained_graph_items",
                allocation.formatted(format_args!("{}", limits.retained_graph_items))?,
            ),
            (
                "rawr.context.retained_graph_bytes",
                allocation.formatted(format_args!("{}", limits.retained_graph_bytes))?,
            ),
            (
                "rawr.context.classified_contract_items",
                allocation.formatted(format_args!("{}", limits.classified_contract_items))?,
            ),
            (
                "rawr.context.classified_contract_bytes",
                allocation.formatted(format_args!("{}", limits.classified_contract_bytes))?,
            ),
            (
                "rawr.context.stage_schema",
                allocation.formatted(format_args!("3"))?,
            ),
        ] {
            insert_derived(&mut values, key, &value, limits, allocation)?;
        }

        // The entry count is an accounting target recorded on the
        // request account; the projection seals every entry.
        allocation.exceeded("environment projection entries", limits.items, values.len());
        allocation.items(values.len())?;
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(values.len())
            .map_err(|error| Error::Request {
                field: "environment projection allocation",
                reason: error.to_string().into_boxed_str(),
            })?;
        for (key, value) in values {
            entries.push(Entry {
                key: key.into_boxed_str(),
                value,
            });
        }
        Ok(Self {
            target: target.into(),
            profile: profile.into(),
            entries: entries.into_boxed_slice(),
        })
    }

    pub(crate) const fn schema(&self) -> u32 {
        SCHEMA
    }

    pub(crate) fn entries(&self) -> impl Iterator<Item = (&str, Option<&str>)> {
        self.entries
            .iter()
            .map(|entry| (entry.key.as_ref(), entry.value.as_deref()))
    }

    pub(super) fn clone_budgeted(&self, allocation: &mut cargo::RequestAllocation) -> Result<Self> {
        allocation.string(&self.target)?;
        allocation.string(&self.profile)?;
        allocation.items(self.entries.len())?;
        for entry in &self.entries {
            allocation.string(&entry.key)?;
            if let Some(value) = entry.value.as_deref() {
                allocation.string(value)?;
            }
        }
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(self.entries.len())
            .map_err(|source| Error::Allocation {
                operation: "environment projection clone",
                source,
            })?;
        for entry in &self.entries {
            entries.push(entry.clone());
        }
        Ok(Self {
            target: self.target.clone(),
            profile: self.profile.clone(),
            entries: entries.into_boxed_slice(),
        })
    }

    pub(crate) fn revalidate(
        &self,
        environment: &Environment,
        command: &'static str,
        limits: &cargo::Limits,
    ) -> Result<()> {
        let current = Self::derive(environment, &self.target, &self.profile, limits)?;
        if &current == self {
            Ok(())
        } else {
            Err(Error::ResolutionDrift {
                command,
                field: "sealed Cargo environment projection",
            })
        }
    }
}

/// Whether one captured variable can alter Cargo/rustc build results: an
/// explicit allowlist of build-affecting Cargo/rustc variables plus the C
/// toolchain variables build scripts read (`CC`, `CFLAGS_<target>`, ...).
/// Diagnostics (`RUST_BACKTRACE`, `RUST_LOG`, `CARGO_LOG`), terminal, network,
/// HTTP, registry credentials and `RUSTUP_*` (the toolchain is bound
/// separately) are absent. Every sealed projection key, `PATH` and `HOME`
/// included, is keyed: the resolution replay rejects a candidate whose
/// projection differs, so a key without them would discard one shell's cached
/// result for another's instead of keeping both.
pub(crate) fn build_relevant(key: &str) -> bool {
    const C_TOOLS: [&str; 10] = [
        "CC",
        "CXX",
        "AR",
        "LD",
        "CFLAGS",
        "CXXFLAGS",
        "CPPFLAGS",
        "LDFLAGS",
        "ARFLAGS",
        "CRATE_CC_NO_DEFAULTS",
    ];
    let c_tool = |name: &str| {
        let name = name
            .strip_prefix("TARGET_")
            .or_else(|| name.strip_prefix("HOST_"))
            .unwrap_or(name);
        C_TOOLS.iter().any(|tool| {
            name == *tool
                || name
                    .strip_prefix(tool)
                    .is_some_and(|rest| rest.starts_with('_'))
        })
    };
    STATIC_KEYS.contains(&key)
        || [
            "CARGO_BUILD_",
            "CARGO_PROFILE_",
            "CARGO_TARGET_",
            "CARGO_FEATURE_",
            "CARGO_UNSTABLE_",
        ]
        .iter()
        .any(|prefix| key.starts_with(prefix))
        || matches!(
            key,
            "CARGO_INCREMENTAL" | "RUSTC" | "RUSTDOC" | "RUSTC_WRAPPER" | "RUSTC_WORKSPACE_WRAPPER"
        )
        || c_tool(key)
}

fn universe_entry(key: &str) -> bool {
    // These values govern where Rawr discovers tools/configuration or how it
    // schedules bounded work.  The selected toolchain commits, exact present
    // config contents, and the request proof bind those authorities elsewhere;
    // their machine-local spellings must not split an otherwise identical
    // compiler-affecting resolution universe.
    !matches!(key, "CARGO_HOME" | "HOME" | "PATH")
        && !matches!(
            key,
            "rawr.context.parallelism_schema"
                | "rawr.context.jobs"
                | "rawr.context.canonical_allocation_items"
                | "rawr.context.canonical_allocation_bytes"
                | "rawr.context.input_proof_bytes"
                | "rawr.context.retained_graph_items"
                | "rawr.context.retained_graph_bytes"
                | "rawr.context.classified_contract_items"
                | "rawr.context.classified_contract_bytes"
                | "rawr.context.stage_schema"
        )
}

fn insert_environment(
    values: &mut BTreeMap<String, Option<Box<str>>>,
    environment: &Environment,
    key: &str,
    limits: &cargo::Limits,
    allocation: &mut cargo::RequestAllocation,
) -> Result<()> {
    bounded("environment projection key", key, limits, allocation);
    let value = environment.value(key).map(|value| keyed_value(key, value));
    if let Some(value) = &value {
        bounded("environment projection value", value, limits, allocation);
    }
    insert(values, key, value.as_deref(), limits, allocation)
}

/// The value one environment variable contributes to keys and projections.
/// `PATH` matters only through the executables it resolves, so it contributes
/// each looked-up tool's resolved canonical path and content digest, never its
/// spelling: shells that order or repeat directories differently but resolve
/// the same tools share one result. Children still inherit the text.
pub(crate) fn keyed_value<'value>(key: &str, value: &'value str) -> std::borrow::Cow<'value, str> {
    if key != "PATH" {
        return value.into();
    }
    const TOOLS: [&str; 15] = [
        "rustup",
        "cargo",
        "rustc",
        "rustdoc",
        "cc",
        "c++",
        "clang",
        "clang++",
        "ld",
        "ar",
        "ranlib",
        "xcrun",
        "pkg-config",
        "cmake",
        "make",
    ];
    let mut keyed = String::new();
    for tool in TOOLS {
        keyed.push_str(tool);
        keyed.push('=');
        if let Some((path, digest)) =
            std::env::split_paths(value).find_map(|dir| resolved(&dir.join(tool)))
        {
            keyed.push_str(&path.to_string_lossy());
            keyed.push('@');
            for byte in digest {
                keyed.push_str(&format!("{byte:02x}"));
            }
        }
        keyed.push('\n');
    }
    keyed.into()
}

/// The canonical path and memoized SHA-256 of one executable PATH candidate.
fn resolved(candidate: &std::path::Path) -> Option<(std::path::PathBuf, [u8; 32])> {
    use crate::file_digest::{self, Domain, Identity};
    use std::os::unix::fs::PermissionsExt as _;
    let metadata = std::fs::metadata(candidate).ok()?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return None;
    }
    let path = std::fs::canonicalize(candidate).ok()?;
    let identity = Identity::of(&metadata);
    if let Some(digest) = file_digest::get(identity, Domain::Sha256) {
        return Some((path, digest));
    }
    let digest: [u8; 32] = Sha256::digest(std::fs::read(&path).ok()?).into();
    if std::fs::metadata(&path)
        .ok()
        .as_ref()
        .and_then(Identity::of)
        == identity
    {
        file_digest::insert(identity, Domain::Sha256, digest);
    }
    Some((path, digest))
}

fn insert_derived(
    values: &mut BTreeMap<String, Option<Box<str>>>,
    key: &str,
    value: &str,
    limits: &cargo::Limits,
    allocation: &mut cargo::RequestAllocation,
) -> Result<()> {
    bounded("environment projection key", key, limits, allocation);
    bounded("environment projection value", value, limits, allocation);
    insert(values, key, Some(value), limits, allocation)
}

fn insert(
    values: &mut BTreeMap<String, Option<Box<str>>>,
    key: &str,
    value: Option<&str>,
    limits: &cargo::Limits,
    allocation: &mut cargo::RequestAllocation,
) -> Result<()> {
    // A new entry past the item target is recorded on the request
    // account and still inserted.
    if !values.contains_key(key) {
        allocation.exceeded(
            "environment projection entries",
            limits.items,
            values.len().saturating_add(1),
        );
    }
    if let Some(previous) = values.get(key) {
        return if previous.as_deref() == value {
            Ok(())
        } else {
            Err(Error::Request {
                field: "environment projection",
                reason: format!("conflicting semantic environment key `{key}`").into_boxed_str(),
            })
        };
    }
    allocation.items(1)?;
    allocation.string(key)?;
    if let Some(value) = value {
        allocation.string(value)?;
    }
    values.insert(key.to_owned(), value.map(Box::from));
    Ok(())
}

fn component(
    value: &str,
    limits: &cargo::Limits,
    allocation: &mut cargo::RequestAllocation,
) -> Result<String> {
    bounded(
        "environment projection component",
        value,
        limits,
        allocation,
    );
    allocation.items(1)?;
    allocation.bytes(value.len())?;
    let mut output = String::new();
    output
        .try_reserve_exact(value.len())
        .map_err(|error| Error::Request {
            field: "environment projection allocation",
            reason: error.to_string().into_boxed_str(),
        })?;
    for byte in value.bytes() {
        output.push(if byte == b'-' {
            '_'
        } else {
            char::from(byte.to_ascii_uppercase())
        });
    }
    Ok(output)
}

fn derived_key(
    prefix: &str,
    component: &str,
    separator: &str,
    suffix: &str,
    limits: &cargo::Limits,
    allocation: &mut cargo::RequestAllocation,
) -> Result<String> {
    let length = prefix
        .len()
        .checked_add(component.len())
        .and_then(|length| length.checked_add(separator.len()))
        .and_then(|length| length.checked_add(suffix.len()))
        .ok_or_else(|| Error::Request {
            field: "environment projection key",
            reason: "derived key length overflow".into(),
        })?;
    allocation.exceeded(
        "environment projection key bytes",
        limits.string_bytes,
        length,
    );
    allocation.items(1)?;
    allocation.bytes(length)?;
    let mut key = String::new();
    key.try_reserve_exact(length)
        .map_err(|error| Error::Request {
            field: "environment projection allocation",
            reason: error.to_string().into_boxed_str(),
        })?;
    key.push_str(prefix);
    key.push_str(component);
    key.push_str(separator);
    key.push_str(suffix);
    Ok(key)
}

/// A projected string's length is an accounting target recorded on the
/// request account; the string is still retained and charged.
fn bounded(
    resource: &'static str,
    value: &str,
    limits: &cargo::Limits,
    allocation: &mut cargo::RequestAllocation,
) {
    allocation.exceeded(resource, limits.string_bytes, value.len());
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use super::Projection;
    use crate::cargo::config::Environment;
    use crate::cargo::Limits;
    use crate::process::parallelism;

    /// A target one unit under an exact total refuses nothing; the
    /// account records the axis with that target and a high water equal to
    /// the exact total the exercise charged.
    fn assert_exceeded(
        record: crate::exceedance::Record,
        name: &str,
        target: usize,
        high_water: usize,
    ) {
        let axis = record
            .axis(name)
            .unwrap_or_else(|| panic!("{name} records the lowered target"));
        assert_eq!(axis.target, target as u64);
        assert_eq!(axis.high_water, high_water as u64);
        assert!(axis.first_requested > axis.target);
        assert!(axis.first_requested <= axis.high_water);
    }

    #[test]
    fn jobs_and_stage_caps_are_exact_freshness_facts() {
        let limits = Limits::standard();
        let seven =
            parallelism::Proof::fixture(NonZeroUsize::new(7).expect("fixture jobs are nonzero"))
                .expect("fixture parallelism seals");
        let eight =
            parallelism::Proof::fixture(NonZeroUsize::new(8).expect("fixture jobs are nonzero"))
                .expect("fixture parallelism seals");
        let projection = Projection::derive_governed(
            &Environment::empty(),
            "aarch64-apple-darwin",
            "dev",
            seven,
            &limits,
        )
        .expect("governed projection derives");
        for (key, value) in [
            ("rawr.context.jobs", "7"),
            ("rawr.context.parallelism_schema", "1"),
            ("rawr.context.stage_schema", "3"),
            ("rawr.context.canonical_allocation_items", "19000000"),
            ("rawr.context.canonical_allocation_bytes", "1207959552"),
            ("rawr.context.input_proof_bytes", "536870912"),
            ("rawr.context.retained_graph_items", "6500000"),
            ("rawr.context.retained_graph_bytes", "536870912"),
            ("rawr.context.classified_contract_items", "6500000"),
            ("rawr.context.classified_contract_bytes", "671088640"),
        ] {
            assert!(projection
                .entries()
                .any(|(name, projected)| name == key && projected == Some(value)));
        }
        assert_eq!(projection.schema(), 10);
        assert!(!projection
            .entries()
            .any(|(name, _)| name.contains("rss") || name == "rawr.context.resource_schema"));

        let changed_jobs = Projection::derive_governed(
            &Environment::empty(),
            "aarch64-apple-darwin",
            "dev",
            eight,
            &limits,
        )
        .expect("changed jobs projection derives");
        assert_ne!(projection, changed_jobs);

        let mut changed_canonical = limits;
        changed_canonical.canonical_items -= 1;
        let changed_canonical = Projection::derive_governed(
            &Environment::empty(),
            "aarch64-apple-darwin",
            "dev",
            seven,
            &changed_canonical,
        )
        .expect("changed canonical authority projects");
        assert_ne!(projection, changed_canonical);

        let mut changed_canonical_bytes = limits;
        changed_canonical_bytes.canonical_bytes -= 1;
        let changed_canonical_bytes = Projection::derive_governed(
            &Environment::empty(),
            "aarch64-apple-darwin",
            "dev",
            seven,
            &changed_canonical_bytes,
        )
        .expect("changed canonical byte authority projects");
        assert_ne!(projection, changed_canonical_bytes);

        let mut changed_input_proof = limits;
        changed_input_proof.input_proof_bytes -= 1;
        let changed_input_proof = Projection::derive_governed(
            &Environment::empty(),
            "aarch64-apple-darwin",
            "dev",
            seven,
            &changed_input_proof,
        )
        .expect("changed input-proof byte authority projects");
        assert_ne!(projection, changed_input_proof);

        for changed in [
            |limits: &mut Limits| limits.retained_graph_items -= 1,
            |limits: &mut Limits| limits.retained_graph_bytes -= 1,
            |limits: &mut Limits| limits.classified_contract_items -= 1,
            |limits: &mut Limits| limits.classified_contract_bytes -= 1,
        ] {
            let mut limits = limits;
            changed(&mut limits);
            let changed_projection = Projection::derive_governed(
                &Environment::empty(),
                "aarch64-apple-darwin",
                "dev",
                seven,
                &limits,
            )
            .expect("changed retained authority projects");
            assert_ne!(projection, changed_projection);
        }
    }

    #[test]
    fn projection_entries_target_profile_and_derived_facts_share_one_exact_ledger() {
        let measure = |limits: &Limits| {
            let parallelism = parallelism::Proof::fixture(
                NonZeroUsize::new(12).expect("fixture jobs are nonzero"),
            )
            .expect("fixture parallelism seals");
            let mut allocation = crate::cargo::RequestAllocation::new(limits);
            let projection = Projection::derive_governed_budgeted(
                &Environment::empty(),
                "aarch64-apple-darwin",
                "dev",
                parallelism,
                limits,
                &mut allocation,
            )?;
            Ok::<_, crate::Error>((allocation, projection))
        };
        let standard = Limits::standard();
        let (usage, projection) = measure(&standard).expect("standard projection measures");
        let mut exact = standard;
        exact.items = usage.items;
        exact.manifest_bytes = usage.bytes;
        let (exact_usage, exact_projection) = measure(&exact).expect("exact ledger seals");
        assert_eq!(usage.items, exact_usage.items);
        assert_eq!(usage.bytes, exact_usage.bytes);
        assert_eq!(projection, exact_projection);
        assert!(!exact_usage.exceedances().is_exceeded());

        // One unit under the exact ledger refuses nothing; the account
        // records the lowered axis with the exact total as its high water.
        let mut item_floor = exact;
        item_floor.items = item_floor.items.saturating_sub(1);
        let (item_usage, item_projection) =
            measure(&item_floor).expect("one item under the ledger still derives");
        assert_eq!(item_projection, projection);
        assert_exceeded(
            item_usage.exceedances(),
            crate::cargo::REQUEST_ITEMS_RESOURCE,
            item_floor.items,
            usage.items,
        );
        let mut byte_floor = exact;
        byte_floor.manifest_bytes = byte_floor.manifest_bytes.saturating_sub(1);
        let (byte_usage, byte_projection) =
            measure(&byte_floor).expect("one byte under the ledger still derives");
        assert_eq!(byte_projection, projection);
        assert_exceeded(
            byte_usage.exceedances(),
            crate::cargo::REQUEST_BYTES_RESOURCE,
            byte_floor.manifest_bytes,
            usage.bytes,
        );
    }

    /// The projection's entry count and string lengths are accounting
    /// targets on the request account: the exact figures record no projection
    /// axis, one under derives the identical projection and records that axis
    /// with its truthful high water, and nothing refuses.
    #[test]
    fn projection_entry_and_string_targets_record_without_refusal() {
        let measure = |limits: &Limits| {
            let parallelism = parallelism::Proof::fixture(
                NonZeroUsize::new(12).expect("fixture jobs are nonzero"),
            )
            .expect("fixture parallelism seals");
            let mut allocation = crate::cargo::RequestAllocation::new(limits);
            let projection = Projection::derive_governed_budgeted(
                &Environment::empty(),
                "aarch64-apple-darwin",
                "dev",
                parallelism,
                limits,
                &mut allocation,
            )
            .expect("the projection derives under every accounting target");
            (allocation.exceedances(), projection)
        };
        let projection_axes = |record: crate::exceedance::Record| {
            record
                .axes()
                .filter(|axis| axis.name.starts_with("environment projection"))
                .copied()
                .collect::<Vec<_>>()
        };
        let standard = Limits::standard();
        let (_, projection) = measure(&standard);
        let entries = projection.entries.len();
        let longest = projection
            .entries
            .iter()
            .map(|entry| {
                entry
                    .key
                    .len()
                    .max(entry.value.as_deref().map_or(0, str::len))
            })
            .max()
            .expect("the projection has entries");
        let mut exact = standard;
        exact.items = entries;
        exact.string_bytes = longest;
        let (record, same) = measure(&exact);
        assert_eq!(same, projection);
        assert!(projection_axes(record).is_empty());

        let mut entry_floor = exact;
        entry_floor.items = entries - 1;
        let (record, same) = measure(&entry_floor);
        assert_eq!(same, projection);
        let axes = projection_axes(record);
        assert_eq!(axes.len(), 1);
        assert_eq!(axes[0].name, "environment projection entries");
        assert_eq!(
            (axes[0].target, axes[0].first_requested, axes[0].high_water),
            ((entries - 1) as u64, entries as u64, entries as u64)
        );

        let mut string_floor = exact;
        string_floor.string_bytes = longest - 1;
        let (record, same) = measure(&string_floor);
        assert_eq!(same, projection);
        let axes = projection_axes(record);
        assert!(!axes.is_empty(), "the longest string records its axis");
        for axis in &axes {
            assert_eq!(axis.target, (longest - 1) as u64);
            assert!(axis.first_requested > axis.target);
            assert!(axis.high_water <= longest as u64);
        }
        assert_eq!(
            axes.iter().map(|axis| axis.high_water).max(),
            Some(longest as u64)
        );
    }

    #[test]
    fn projection_clone_charges_before_owning_every_retained_fact() {
        let standard = Limits::standard();
        let projection = Projection::derive(
            &Environment::empty(),
            "aarch64-apple-darwin",
            "dev",
            &standard,
        )
        .expect("fixture projection derives");
        let measure = |limits: &Limits| {
            let mut allocation = crate::cargo::RequestAllocation::new(limits);
            let cloned = projection.clone_budgeted(&mut allocation)?;
            Ok::<_, crate::Error>((allocation, cloned))
        };
        let (usage, cloned) = measure(&standard).expect("standard clone measures");
        assert_eq!(projection, cloned);

        let mut exact = standard;
        exact.items = usage.items;
        exact.manifest_bytes = usage.bytes;
        let (exact_usage, exact_clone) = measure(&exact).expect("exact clone seals");
        assert_eq!(projection, exact_clone);
        assert!(!exact_usage.exceedances().is_exceeded());

        // One unit under the exact clone ledger refuses nothing; the
        // account records the lowered axis with the exact total as high water.
        let mut item_floor = exact;
        item_floor.items = item_floor.items.saturating_sub(1);
        let (item_usage, item_clone) =
            measure(&item_floor).expect("one item under the clone ledger still clones");
        assert_eq!(projection, item_clone);
        assert_exceeded(
            item_usage.exceedances(),
            crate::cargo::REQUEST_ITEMS_RESOURCE,
            item_floor.items,
            usage.items,
        );
        let mut byte_floor = exact;
        byte_floor.manifest_bytes = byte_floor.manifest_bytes.saturating_sub(1);
        let (byte_usage, byte_clone) =
            measure(&byte_floor).expect("one byte under the clone ledger still clones");
        assert_eq!(projection, byte_clone);
        assert_exceeded(
            byte_usage.exceedances(),
            crate::cargo::REQUEST_BYTES_RESOURCE,
            byte_floor.manifest_bytes,
            usage.bytes,
        );
    }

    #[test]
    fn semantic_values_preserve_unset_empty_and_changed() {
        let limits = Limits::standard();
        let unset = Projection::derive(
            &Environment::empty(),
            "aarch64-apple-darwin",
            "dev",
            &limits,
        )
        .expect("unset projection derives");
        let empty = Projection::derive(
            &Environment::from_pairs([("RUSTFLAGS", "")]).expect("empty value seals"),
            "aarch64-apple-darwin",
            "dev",
            &limits,
        )
        .expect("empty projection derives");
        let changed = Projection::derive(
            &Environment::from_pairs([("RUSTFLAGS", "-Ctarget-cpu=native")])
                .expect("semantic value seals"),
            "aarch64-apple-darwin",
            "dev",
            &limits,
        )
        .expect("changed projection derives");
        assert_ne!(unset, empty);
        assert_ne!(empty, changed);
        assert!(unset
            .entries()
            .any(|(key, value)| key == "RUSTFLAGS" && value.is_none()));
        assert!(empty
            .entries()
            .any(|(key, value)| key == "RUSTFLAGS" && value == Some("")));
    }

    #[test]
    fn target_and_profile_families_are_derived_from_the_request() {
        let environment = Environment::from_pairs([
            ("CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER", "clang"),
            ("CARGO_PROFILE_DEVELOPMENT_OPT_LEVEL", "2"),
        ])
        .expect("derived-family values seal");
        let projection = Projection::derive(
            &environment,
            "aarch64-apple-darwin",
            "development",
            &Limits::standard(),
        )
        .expect("derived families project");
        assert!(projection.entries().any(|(key, value)| {
            key == "CARGO_TARGET_AARCH64_APPLE_DARWIN_LINKER" && value == Some("clang")
        }));
        assert!(projection.entries().any(|(key, value)| {
            key == "CARGO_PROFILE_DEVELOPMENT_OPT_LEVEL" && value == Some("2")
        }));
    }

    #[test]
    fn config_and_darwin_toolchain_inputs_are_explicitly_projected() {
        let environment = Environment::from_pairs([
            ("CARGO_HOME", ""),
            ("HOME", "/Users/tester"),
            ("PATH", "/usr/bin:/bin"),
            ("SDKROOT", ""),
            (
                "DEVELOPER_DIR",
                "/Applications/Xcode.app/Contents/Developer",
            ),
            ("MACOSX_DEPLOYMENT_TARGET", "15.0"),
        ])
        .expect("toolchain environment seals");
        let projection = Projection::derive(
            &environment,
            "aarch64-apple-darwin",
            "dev",
            &Limits::standard(),
        )
        .expect("toolchain projection derives");
        for (key, value) in [
            ("CARGO_HOME", ""),
            ("HOME", "/Users/tester"),
            ("SDKROOT", ""),
            (
                "DEVELOPER_DIR",
                "/Applications/Xcode.app/Contents/Developer",
            ),
            ("MACOSX_DEPLOYMENT_TARGET", "15.0"),
        ] {
            assert!(projection
                .entries()
                .any(|(name, projected)| name == key && projected == Some(value)));
        }
    }

    #[test]
    fn undeclared_operational_and_importer_overridden_values_are_excluded() {
        let limits = Limits::standard();
        let baseline = Projection::derive(
            &Environment::empty(),
            "aarch64-apple-darwin",
            "dev",
            &limits,
        )
        .expect("baseline projection derives");
        let environment = Environment::from_pairs([
            ("ARBITRARY_SENTINEL", "changed"),
            ("CARGO_INCREMENTAL", "1"),
            ("CARGO_REGISTRY_TOKEN", "secret"),
            ("CARGO_TERM_COLOR", "always"),
            ("HTTPS_PROXY", "https://proxy.invalid"),
            ("RUSTC", "/ignored/rustc"),
        ])
        .expect("operational environment seals");
        let changed = Projection::derive(&environment, "aarch64-apple-darwin", "dev", &limits)
            .expect("operational projection derives");
        assert_eq!(baseline, changed);
    }

    #[test]
    fn unmodeled_and_dynamic_profile_environment_rejects() {
        for key in [
            "CARGO_PROFILE_CUSTOM_INHERITS",
            "CARGO_PROFILE_DEV_PACKAGE_EXAMPLE_OPT_LEVEL",
            "CARGO_PROFILE_DEV_UNKNOWN",
            "CARGO_PROFILE_RELEASE_OPT_LEVEL",
        ] {
            let environment = Environment::from_pairs([(key, "dev")])
                .expect("profile environment entry seals before semantic classification");
            let error = Projection::derive(
                &environment,
                "aarch64-apple-darwin",
                "dev",
                &Limits::standard(),
            )
            .expect_err("unmodeled profile environment must reject");
            assert!(matches!(
                error,
                crate::Error::Request {
                    field: "Cargo profile environment authority",
                    ..
                }
            ));
        }
    }
}

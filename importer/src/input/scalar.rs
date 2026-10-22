//! Borrowed scalar observations emitted in the order of their owning inputs.

use std::path::Path;

use super::proof::{Recipe, Role};
use super::{Allocation, HexDigest};
use crate::cargo::{config, internal, toolchain, Limits};
use crate::Result;

pub(in crate::input) mod account;

use account::Account;

/// One scalar payload, without filesystem freshness or execution authority.
pub(crate) enum Value<'value> {
    Fact {
        role: Role,
        package_id: Option<&'value str>,
        name: &'value str,
        value: &'value str,
    },
    EnvironmentProjection {
        schema: u32,
        key: &'value str,
        value: Option<&'value str>,
    },
}

impl Value<'_> {
    pub(super) fn retain(self, allocation: &mut Allocation) -> Result<Recipe> {
        match self {
            Self::Fact {
                role,
                package_id: None,
                name,
                value,
            } => Recipe::fact(role, name, value, allocation),
            Self::Fact {
                role,
                package_id: Some(package_id),
                name,
                value,
            } => Recipe::package_fact(role, package_id, name, value, allocation),
            Self::EnvironmentProjection { schema, key, value } => {
                Recipe::projection(schema, key, value, allocation)
            }
        }
    }
}

/// Emit the sealed semantic environment followed by its exact config identity.
pub(crate) fn config<A: Account>(
    projection: &config::projection::Projection,
    provenance: &[u8; 32],
    limits: &Limits,
    allocation: &mut A,
    mut emit: impl FnMut(Value<'_>, &mut A) -> std::result::Result<(), A::Error>,
) -> std::result::Result<(), A::Error> {
    allocation.check()?;
    for (key, value) in projection.entries() {
        allocation.bounded(
            "Cargo environment projection key",
            limits.string_bytes,
            key.len(),
        )?;
        if let Some(value) = value {
            allocation.bounded(
                "Cargo environment projection value",
                limits.string_bytes,
                value.len(),
            )?;
        }
        emit(
            Value::EnvironmentProjection {
                schema: projection.schema(),
                key,
                value,
            },
            allocation,
        )?;
    }
    let text = account::provenance(allocation, provenance)?;
    emit(fact(Role::Config, "config-provenance", &text), allocation)
}

/// Emit the exact internal package recipe, including its native source root.
pub(crate) fn internal<A: Account>(
    internal: &internal::Recipe,
    allocation: &mut A,
    mut emit: impl FnMut(Value<'_>, &mut A) -> std::result::Result<(), A::Error>,
) -> std::result::Result<(), A::Error> {
    allocation.check()?;
    let schema = account::formatted(allocation, format_args!("{}", internal.schema()))?;
    emit(
        fact(Role::Toolchain, "internal-recipe-schema", &schema),
        allocation,
    )?;
    for (name, value) in [
        ("internal-package-id", internal.package_id()),
        ("internal-source", internal.source()),
        (
            "internal-root",
            allocation.path_text("internal Rawr root", internal.root())?,
        ),
        ("internal-version", internal.version()),
        ("internal-features", "rust-import,std"),
        ("internal-default-features", "false"),
        ("internal-profile-debug", "0"),
    ] {
        emit(fact(Role::Toolchain, name, value), allocation)?;
    }
    Ok(())
}

/// Emit an already-attested toolchain without discovering tools or launching them.
pub(crate) fn toolchain<A: Account>(
    proof: &toolchain::Proof,
    allocation: &mut A,
    mut emit: impl FnMut(Value<'_>, &mut A) -> std::result::Result<(), A::Error>,
) -> std::result::Result<(), A::Error> {
    allocation.check()?;
    for (name, value) in [
        ("channel", proof.channel()),
        ("rustc-release", proof.rustc_release()),
        ("rustc-commit", proof.rustc_commit()),
        ("rustdoc-release", proof.rustdoc_release()),
        ("rustdoc-commit", proof.rustdoc_commit()),
        ("cargo-release", proof.cargo_release()),
        ("cargo-commit", proof.cargo_commit()),
        ("host", proof.host()),
        ("installed-target", proof.target()),
    ] {
        emit(fact(Role::Toolchain, name, value), allocation)?;
    }
    for ((path_name, length_name, digest_name), (_name, path, length, digest)) in [
        ("rustup-path", "rustup-length", "rustup-digest"),
        ("cargo-path", "cargo-length", "cargo-digest"),
        ("rustc-path", "rustc-length", "rustc-digest"),
        ("rustdoc-path", "rustdoc-length", "rustdoc-digest"),
    ]
    .into_iter()
    .zip(proof.executables())
    {
        let path = allocation.executable_path(path)?;
        let length = account::formatted(allocation, format_args!("{length}"))?;
        let digest = account::formatted(allocation, format_args!("{}", HexDigest(digest)))?;
        emit(fact(Role::Toolchain, path_name, path), allocation)?;
        emit(fact(Role::Toolchain, length_name, &length), allocation)?;
        emit(fact(Role::Toolchain, digest_name, &digest), allocation)?;
    }
    #[cfg(feature = "rustc-driver")]
    {
        let path = allocation.executable_path(proof.driver_path())?;
        emit(
            fact(Role::Toolchain, "metadata-probe-driver-path", path),
            allocation,
        )?;
        let digest = account::formatted(
            allocation,
            format_args!("{}", HexDigest(proof.driver_digest())),
        )?;
        let length = account::formatted(allocation, format_args!("{}", proof.driver_length()))?;
        emit(
            fact(Role::Toolchain, "metadata-probe-driver-length", &length),
            allocation,
        )?;
        emit(
            fact(Role::Toolchain, "metadata-probe-driver-digest", &digest),
            allocation,
        )?;
    }
    for component in proof.components() {
        emit(fact(Role::Toolchain, "component", component), allocation)?;
    }
    Ok(())
}

/// Emit the request's exact target and profile spellings.
pub(crate) fn selection<A: Account>(
    target: &str,
    profile: &str,
    allocation: &mut A,
    mut emit: impl FnMut(Value<'_>, &mut A) -> std::result::Result<(), A::Error>,
) -> std::result::Result<(), A::Error> {
    allocation.check()?;
    emit(fact(Role::Target, "target", target), allocation)?;
    emit(fact(Role::Profile, "profile", profile), allocation)
}

/// Emit one package root after that package's manifest and build-script inputs.
pub(crate) fn package<A: Account>(
    package_id: &str,
    root: &Path,
    allocation: &mut A,
    mut emit: impl FnMut(Value<'_>, &mut A) -> std::result::Result<(), A::Error>,
) -> std::result::Result<(), A::Error> {
    allocation.check()?;
    let name = account::formatted(allocation, format_args!("package:{package_id}"))?;
    allocation.package_path(root)?;
    let value = allocation.path_text("package root", root)?;
    emit(
        Value::Fact {
            role: Role::DependencySource,
            package_id: Some(package_id),
            name: &name,
            value,
        },
        allocation,
    )
}

fn fact<'value>(role: Role, name: &'value str, value: &'value str) -> Value<'value> {
    Value::Fact {
        role,
        package_id: None,
        name,
        value,
    }
}

#[cfg(test)]
mod tests;

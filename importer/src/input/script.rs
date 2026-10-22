//! Borrowed output recipes from one already-associated Build script.

use std::ffi::OsString;
use std::path::Path;

use super::scalar::account::{self, Account};
use super::{build, declaration, proof, Allocation};
use crate::Result;

/// Payloads only: neither variant establishes current input or execution proof.
pub(super) enum Value<'value> {
    SavedOutput {
        package_id: &'value str,
        out_dir: &'value Path,
    },
    Directive {
        package_id: &'value str,
        execution: &'value str,
        index: usize,
        value: &'value str,
    },
    DeclaredPath {
        package_id: &'value str,
        path: &'value Path,
    },
    Environment {
        package_id: &'value str,
        execution: &'value str,
        key: &'value str,
        value: Option<&'value str>,
    },
    Directory {
        package_id: &'value str,
        path: &'value Path,
    },
    BuildMessage {
        package_id: &'value str,
        execution: &'value str,
        name: &'value str,
        value: &'value str,
    },
}

impl Value<'_> {
    /// The cold caller admits its recipe slot before retaining these fields.
    pub(super) fn retain(self, allocation: &mut Allocation) -> Result<proof::Recipe> {
        match self {
            Self::SavedOutput {
                package_id,
                out_dir,
            } => proof::Recipe::saved_output(package_id, out_dir, allocation),
            Self::Directive {
                package_id,
                execution,
                index,
                value,
            } => proof::Recipe::directive(package_id, execution, index, value, allocation),
            Self::DeclaredPath { package_id, path } => proof::Recipe::package_path(
                proof::Role::DeclaredInput,
                package_id,
                path,
                allocation,
            ),
            Self::Environment {
                package_id,
                execution,
                key,
                value,
            } => proof::Recipe::environment(package_id, execution, key, value, allocation),
            Self::Directory { package_id, path } => proof::Recipe::package_directory(
                proof::Role::OutDirectory,
                package_id,
                path,
                allocation,
            ),
            Self::BuildMessage {
                package_id,
                execution,
                name,
                value,
            } => proof::Recipe::build_message(package_id, execution, name, value, allocation),
        }
    }
}

/// Emit already-acquired declarations in recipe order, returning the borrowed
/// execution spelling for the caller's subsequent fallback and output handling.
pub(super) fn declarations<'script, A: Account>(
    script: &'script build::Script,
    package: &build::Package,
    declarations: &declaration::Declarations,
    cargo_env_overrides: bool,
    environment: &[(OsString, OsString)],
    allocation: &mut A,
    mut emit: impl FnMut(Value<'_>, &mut A) -> std::result::Result<(), A::Error>,
) -> std::result::Result<&'script str, A::Error> {
    allocation.check()?;
    allocation.package_path(&script.out_dir)?;
    let execution = allocation.path_text("build-script execution OUT_DIR", &script.out_dir)?;
    if cargo_env_overrides && !declarations.environment.is_empty() {
        return Err(allocation.failure(account::Failure::CargoEnvironmentOverrides));
    }
    emit(
        Value::SavedOutput {
            package_id: &script.package_id,
            out_dir: &script.out_dir,
        },
        allocation,
    )?;
    for (index, directive) in declarations.directives.iter().enumerate() {
        emit(
            Value::Directive {
                package_id: &script.package_id,
                execution,
                index,
                value: directive,
            },
            allocation,
        )?;
    }
    for declared in &declarations.paths {
        let path = allocation.declared_path(&package.root, declared)?;
        emit(
            Value::DeclaredPath {
                package_id: &script.package_id,
                path: &path,
            },
            allocation,
        )?;
    }
    for key in &declarations.environment {
        emit(
            Value::Environment {
                package_id: &script.package_id,
                execution,
                key,
                value: super::environment_value(environment, key),
            },
            allocation,
        )?;
    }
    Ok(execution)
}

/// Package fallback depends on rerun declarations, independently of directives.
pub(super) fn fallback(declarations: &declaration::Declarations) -> bool {
    declarations.paths.is_empty() && declarations.environment.is_empty()
}

/// Emit OUT_DIR, cfg messages, then environment messages in their Build order.
/// The caller has already handled declarations and fallback package selection.
pub(super) fn outputs<A: Account>(
    script: &build::Script,
    execution: &str,
    allocation: &mut A,
    mut emit: impl FnMut(Value<'_>, &mut A) -> std::result::Result<(), A::Error>,
) -> std::result::Result<(), A::Error> {
    allocation.check()?;
    emit(
        Value::Directory {
            package_id: &script.package_id,
            path: &script.out_dir,
        },
        allocation,
    )?;
    for cfg in &script.cfg {
        emit(
            Value::BuildMessage {
                package_id: &script.package_id,
                execution,
                name: "cfg",
                value: cfg,
            },
            allocation,
        )?;
    }
    for (key, value) in &script.environment {
        // This temporary precedes the sink's recipe-slot admission, just as in
        // cold capture. Only the callback may retain a separately charged copy.
        let name = account::formatted(allocation, format_args!("env:{key}"))?;
        emit(
            Value::BuildMessage {
                package_id: &script.package_id,
                execution,
                name: &name,
                value,
            },
            allocation,
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;

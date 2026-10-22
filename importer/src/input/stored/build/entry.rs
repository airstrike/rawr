use std::path::Path;

use super::{Budget, Entry, Package, Result};
use crate::input::declaration::Declarations;
use crate::input::proof::Role;
use crate::input::stored::{observe, view::View};
use crate::input::{artifact, replay, script};

pub(super) fn artifact(
    value: artifact::Value<'_>,
    base: &Path,
    budget: &mut Budget<'_>,
) -> Result<Entry> {
    match value {
        artifact::Value::File { package_id, path } => observe::entry::<Package>(
            View::File {
                role: Role::DependencySource,
                package_id: Some(package_id),
                path,
            },
            base,
            budget,
        ),
        artifact::Value::Environment {
            package_id,
            execution,
            key,
            value,
        } => environment(package_id, execution, key, value, budget),
    }
}

pub(super) fn script(
    value: script::Value<'_>,
    declarations: &Declarations,
    base: &Path,
    budget: &mut Budget<'_>,
) -> Result<Entry> {
    match value {
        script::Value::SavedOutput { out_dir, .. } => {
            replay::observed::saved_output(declarations, out_dir, budget)
        }
        script::Value::Directive {
            package_id,
            execution,
            index,
            value,
        } => Ok(Entry::Directive {
            package_id: scalar(package_id, budget)?,
            execution: scalar(execution, budget)?,
            index,
            value: scalar(value, budget)?,
        }),
        script::Value::DeclaredPath { package_id, path } => observe::entry::<Package>(
            View::Path {
                role: Role::DeclaredInput,
                package_id: Some(package_id),
                path,
            },
            base,
            budget,
        ),
        script::Value::Environment {
            package_id,
            execution,
            key,
            value,
        } => environment(package_id, execution, key, value, budget),
        script::Value::Directory { package_id, path } => observe::entry::<Package>(
            View::Directory {
                role: Role::OutDirectory,
                package_id: Some(package_id),
                path,
            },
            base,
            budget,
        ),
        script::Value::BuildMessage {
            package_id,
            execution,
            name,
            value,
        } => Ok(Entry::BuildMessage {
            package_id: scalar(package_id, budget)?,
            execution: scalar(execution, budget)?,
            name: scalar(name, budget)?,
            source: None,
            value: scalar(value, budget)?,
        }),
    }
}

fn environment(
    package_id: &str,
    execution: &str,
    key: &str,
    value: Option<&str>,
    budget: &mut Budget<'_>,
) -> Result<Entry> {
    Ok(Entry::Environment {
        package_id: scalar(package_id, budget)?,
        execution: scalar(execution, budget)?,
        key: scalar(key, budget)?,
        value: value.map(|value| scalar(value, budget)).transpose()?,
    })
}

fn scalar(value: &str, budget: &mut Budget<'_>) -> Result<Box<str>> {
    budget.limited(
        "input owned string bytes",
        budget.limits.string_bytes,
        value.len(),
    )?;
    budget.owned_text(value)
}

use super::{Error, Recipe, Result};
use crate::input::proof::Role;
use crate::input::{artifact, build, script};

pub(super) fn header(
    recipe: &Recipe,
    artifact: &build::Artifact,
    package: &build::Package,
    scripts: &[build::Script],
) -> Result<()> {
    let Recipe::DepInfo {
        package_id,
        path,
        package_root,
        out_directories,
    } = recipe
    else {
        return differs();
    };
    equal(
        package_id == &artifact.package_id
            && path.as_os_str() == artifact.dep_info.as_os_str()
            && package_root.as_os_str() == package.root.as_os_str()
            && out_directories
                .iter()
                .map(|path| path.as_os_str())
                .eq(scripts
                    .iter()
                    .filter(|script| script.package_id == artifact.package_id)
                    .map(|script| script.out_dir.as_os_str())),
    )
}

pub(super) fn artifact(value: &artifact::Value<'_>, recipe: &Recipe) -> Result<()> {
    equal(match (value, recipe) {
        (
            artifact::Value::File { package_id, path },
            Recipe::File {
                role,
                package_id: stored_package,
                path: stored_path,
            },
        ) => {
            *role == Role::DependencySource
                && stored_package.as_deref() == Some(*package_id)
                && path.as_os_str() == stored_path.as_os_str()
        }
        (
            artifact::Value::Environment {
                package_id,
                execution,
                key,
                value,
            },
            Recipe::Environment {
                package_id: stored_package,
                execution: stored_execution,
                key: stored_key,
                value: stored_value,
            },
        ) => {
            (*package_id, *execution, *key, *value)
                == (
                    stored_package.as_ref(),
                    stored_execution.as_ref(),
                    stored_key.as_ref(),
                    stored_value.as_deref(),
                )
        }
        _ => false,
    })
}

pub(super) fn script(value: &script::Value<'_>, recipe: &Recipe) -> Result<()> {
    equal(match (value, recipe) {
        (
            script::Value::SavedOutput {
                package_id,
                out_dir,
            },
            Recipe::SavedOutput {
                package_id: stored_package,
                out_dir: stored_directory,
            },
        ) => {
            *package_id == stored_package.as_ref()
                && out_dir.as_os_str() == stored_directory.as_os_str()
        }
        (
            script::Value::Directive {
                package_id,
                execution,
                index,
                value,
            },
            Recipe::Directive {
                package_id: stored_package,
                execution: stored_execution,
                index: stored_index,
                value: stored_value,
            },
        ) => {
            (*package_id, *execution, *index, *value)
                == (
                    stored_package.as_ref(),
                    stored_execution.as_ref(),
                    *stored_index,
                    stored_value.as_ref(),
                )
        }
        (
            script::Value::DeclaredPath { package_id, path },
            Recipe::Path {
                role,
                package_id: stored_package,
                path: stored_path,
            },
        ) => {
            *role == Role::DeclaredInput
                && stored_package.as_deref() == Some(*package_id)
                && path.as_os_str() == stored_path.as_os_str()
        }
        (
            script::Value::Environment {
                package_id,
                execution,
                key,
                value,
            },
            Recipe::Environment {
                package_id: stored_package,
                execution: stored_execution,
                key: stored_key,
                value: stored_value,
            },
        ) => {
            (*package_id, *execution, *key, *value)
                == (
                    stored_package.as_ref(),
                    stored_execution.as_ref(),
                    stored_key.as_ref(),
                    stored_value.as_deref(),
                )
        }
        (
            script::Value::Directory { package_id, path },
            Recipe::Directory {
                role,
                package_id: stored_package,
                path: stored_path,
            },
        ) => {
            *role == Role::OutDirectory
                && stored_package.as_deref() == Some(*package_id)
                && path.as_os_str() == stored_path.as_os_str()
        }
        (
            script::Value::BuildMessage {
                package_id,
                execution,
                name,
                value,
            },
            Recipe::BuildMessage {
                package_id: stored_package,
                execution: stored_execution,
                name: stored_name,
                value: stored_value,
            },
        ) => {
            (*package_id, *execution, *name, *value)
                == (
                    stored_package.as_ref(),
                    stored_execution.as_ref(),
                    stored_name.as_ref(),
                    stored_value.as_ref(),
                )
        }
        _ => false,
    })
}

fn equal(equal: bool) -> Result<()> {
    if equal {
        Ok(())
    } else {
        differs()
    }
}

fn differs() -> Result<()> {
    Err(Error::Encoding("stored Build recipe differs"))
}

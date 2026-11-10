//! The probe of a grown demand over the artifacts an earlier probe of the
//! same build read (#1618).
//!
//! A demand round of one build changes only what the probe extracts. The
//! driver grows each selected crate's table in the reuse directory with the
//! units the new demand reaches, and the staged tables then replay the
//! demand as shared bindings: no harness, no build, no copy and no bundle.
//! The probe reads the sealed bundle's artifacts the earlier plan names;
//! its protocol sidecars are not kept, since the tables answer the demand.

use std::fs;
use std::path::{Path, PathBuf};

use crate::cargo;
use crate::process::{self, Program};
use crate::{Error, Result};

/// One bound probe plan of a build, which a later demand runs again.
pub(crate) struct Reprobe {
    plan: Box<[u8]>,
    reuse: PathBuf,
}

impl Reprobe {
    /// The plan `plan` bound, whose tables the driver keeps in `reuse`.
    pub(super) fn of(plan: &[u8], reuse: &Path) -> Self {
        Self {
            plan: plan.into(),
            reuse: reuse.to_path_buf(),
        }
    }

    /// Run the probe again in `directory`, for `demand`: the driver grows
    /// the tables of the reuse directory, and the sidecars it writes go.
    pub(crate) fn run(
        &self,
        native: &impl process::ProductionReplay,
        demand: &[cargo::DemandPath],
        directory: &Path,
        limits: &cargo::Limits,
    ) -> Result<()> {
        let invalid = |reason: String| Error::Extraction {
            code: "reprobe-plan",
            reason: reason.into_boxed_str(),
        };
        let mut plan = serde_json::from_slice::<serde_json::Value>(&self.plan)
            .map_err(|error| invalid(error.to_string()))?;
        let fields = plan
            .as_object_mut()
            .ok_or_else(|| invalid("the plan is not an object".into()))?;
        // As the plan's own encoding has it: an empty demand is omitted.
        if demand.is_empty() {
            fields.remove("demand");
        } else {
            fields.insert(
                "demand".into(),
                serde_json::to_value(demand).map_err(|error| invalid(error.to_string()))?,
            );
        }
        let io = |operation: &'static str, path: &Path| {
            let path = path.to_path_buf();
            move |source| Error::Io {
                operation,
                path,
                source,
            }
        };
        let _ = fs::remove_dir_all(directory);
        let protocol = directory.join(super::PROTOCOL_DIRECTORY);
        fs::create_dir_all(&protocol).map_err(io("create reprobe directory", &protocol))?;
        let plan_file = directory.join(super::PLAN_FILE);
        let source = directory.join(super::PROBE_SOURCE_FILE);
        let plan = serde_json::to_vec(&plan).map_err(|error| invalid(error.to_string()))?;
        fs::write(&plan_file, plan).map_err(io("write reprobe plan", &plan_file))?;
        fs::write(&source, super::PROBE_SOURCE).map_err(io("write reprobe source", &source))?;
        let arguments = vec![
            "--probe".into(),
            native.rustc().as_os_str().to_owned(),
            plan_file.into_os_string(),
            protocol.into_os_string(),
            source.into_os_string(),
            self.reuse.as_os_str().to_owned(),
        ];
        let environment = process::driver_library(native.rustc(), &[])
            .into_iter()
            .collect();
        let command = process::Command::probe(arguments, directory.to_path_buf(), environment)?;
        crate::progress::probed();
        native
            .run(&command, limits.process_limits())?
            .checked(Program::Driver)?;
        let _ = fs::remove_dir_all(directory);
        Ok(())
    }
}

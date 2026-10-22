//! One declaration's completed fields, or those fields awaiting native evidence.
use super::{artifact, record};

pub(in super::super::super) enum Progress<'a, 'p, 's> {
    Complete(Converted<'p, 's>),
    Canonical(Pending<'a, 'p, 's>),
}

pub(in super::super::super) struct Converted<'p, 's> {
    prefix: record::BeforeArtifact<'p, 's>,
    artifact: Option<artifact::Artifact>,
}

impl<'p, 's> Converted<'p, 's> {
    pub(in super::super::super) fn fields(&self) -> &record::BeforeArtifact<'p, 's> {
        &self.prefix
    }
    pub(in super::super::super) fn has_artifact(&self) -> bool {
        self.artifact.is_some()
    }
    /// Called after replacement warnings, matching Cargo's set_version_req.
    pub(in super::super::super) fn replace_version<A: rawr_toml::document::Admission>(
        &mut self,
        version: &crate::input::membership::manifest::spec::Full,
        session: &mut crate::input::membership::manifest::session::Session<'_, A>,
    ) -> crate::input::membership::manifest::Step<()> {
        let requirement =
            crate::input::membership::manifest::scalar::requirement::Requirement::exact(
                version, session,
            )?;
        self.prefix.replace_requirement(requirement);
        Ok(())
    }
}

/// No completion entry point exists until native evidence is associated.
pub(in super::super::super) struct Pending<'a, 'p, 's> {
    prefix: record::BeforeArtifact<'p, 's>,
    artifact: artifact::Pending<'a>,
}

impl<'p, 's> Pending<'_, 'p, 's> {
    pub(in super::super::super) fn fields(&self) -> &record::BeforeArtifact<'p, 's> {
        &self.prefix
    }
    pub(in super::super::super) fn spelling(&self) -> &str {
        self.artifact.spelling()
    }
    pub(in super::super::super) fn dependency_name(&self) -> &str {
        self.artifact.dependency_name()
    }
}

pub(super) fn retain<'a, 'p, 's>(
    prefix: record::BeforeArtifact<'p, 's>,
    artifact: artifact::Progress<'a>,
) -> Progress<'a, 'p, 's> {
    match artifact {
        artifact::Progress::Complete(artifact) => {
            Progress::Complete(Converted { prefix, artifact })
        }
        artifact::Progress::Canonical(artifact) => {
            Progress::Canonical(Pending { prefix, artifact })
        }
    }
}

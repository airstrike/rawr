//! Typestate-sealed generated runner manifests.

pub mod artifact;
mod build;

#[cfg(all(test, feature = "test-fixtures"))]
pub(crate) use build::fixture_build;
pub mod manifest;
pub(crate) mod overlay;

pub use build::Error as BuildError;

/// A manifest whose normalized source exists but has not been linked.
pub struct Generated {
    pub(crate) _private: (),
}

/// A manifest whose executable has been independently sealed.
pub struct Built {
    native: Native,
    read_set: Option<crate::node::read_set::Prepared>,
}

pub(crate) enum Native {
    Legacy {
        artifact: artifact::BuiltArtifactProof,
        _pin: crate::cache::pins::Pin,
    },
    Shared {
        artifact: artifact::BuiltArtifactProof,
        retained: artifact::retained::Proof,
        _pin: crate::store::bundle::Guard,
    },
}

impl Native {
    fn revalidate(&self) -> Result<(), artifact::Error> {
        match self {
            Self::Legacy { artifact, .. } => artifact.revalidate(),
            Self::Shared {
                artifact, retained, ..
            } => {
                retained.revalidate()?;
                artifact.revalidate_budgeted(retained.budget())
            }
        }
    }
}

/// Phase-indexed ownership of one exact runner request and its generated files.
pub struct Manifest<P, B> {
    pub(crate) request: crate::surface::Request<B>,
    pub(crate) native: crate::input::proof::Proof,
    pub(crate) profile: crate::profile::Proof,
    pub(crate) revision: crate::supervisor::Revision,
    pub(crate) generated: manifest::GeneratedManifest,
    pub(crate) files: Box<[manifest::File]>,
    pub(crate) chain: Option<crate::node::Chain>,
    pub(crate) overlay: Option<overlay::Overlay>,
    pub(crate) phase: P,
}

impl<B> Manifest<Generated, B> {
    /// Borrow the exact pre-link manifest embedded in the runner.
    #[must_use]
    pub const fn generated_manifest(&self) -> &manifest::GeneratedManifest {
        &self.generated
    }

    /// Iterate normalized generated files in canonical path order.
    pub fn files(&self) -> impl ExactSizeIterator<Item = &manifest::File> {
        self.files.iter()
    }
}

impl<B> Manifest<Built, B> {
    /// Bind the startup snapshot and retain its complete Prepared allocation.
    /// This is ownership only; watcher and pre-spawn current checks still apply.
    pub fn retain_startup(
        &mut self,
        snapshot: &crate::supervisor::snapshot::Bytes,
    ) -> Result<(), crate::node::Error> {
        if !snapshot.matches(self.request.snapshot) {
            return Err(crate::node::Error::State("runner snapshot binding changed"));
        }
        if let Some(read_set) = &mut self.phase.read_set {
            read_set.retain_startup(snapshot)?;
        }
        Ok(())
    }

    pub(crate) fn bind_startup_snapshot(
        &mut self,
        snapshot: &crate::supervisor::snapshot::Bytes,
    ) -> Result<(), crate::node::Error> {
        if !snapshot.matches(self.request.snapshot) {
            return Err(crate::node::Error::State("runner snapshot binding changed"));
        }
        if let Some(read_set) = &mut self.phase.read_set {
            read_set.bind_startup_snapshot(snapshot)?;
        }
        Ok(())
    }

    /// Bind current Rawr source freshness before obtaining any admission pin.
    pub fn bind_snapshot(
        &mut self,
        snapshot: &crate::supervisor::snapshot::Bytes,
    ) -> Result<(), crate::node::Error> {
        if !snapshot.matches(self.request.snapshot) {
            return Err(crate::node::Error::State("runner snapshot binding changed"));
        }
        if let Some(read_set) = &mut self.phase.read_set {
            read_set.bind_snapshot(snapshot)?;
        }
        Ok(())
    }

    pub(crate) fn accept_hot(
        &mut self,
        current: Manifest<Generated, B>,
        read_set: crate::node::read_set::Prepared,
    ) {
        self.request = current.request;
        self.native = current.native;
        self.profile = current.profile;
        self.revision = current.revision;
        self.generated = current.generated;
        self.files = current.files;
        self.chain = current.chain;
        self.overlay = current.overlay;
        self.phase.read_set = Some(read_set);
    }

    /// Borrow the independently hashed final executable proof.
    #[must_use]
    pub const fn artifact(&self) -> &artifact::BuiltArtifactProof {
        match &self.phase.native {
            Native::Legacy { artifact, .. } | Native::Shared { artifact, .. } => artifact,
        }
    }

    pub(crate) fn native_identity(&self) -> [u8; 32] {
        match &self.phase.native {
            Native::Legacy { artifact, .. } => artifact.digest(),
            Native::Shared {
                artifact, retained, ..
            } => retained.native_identity(artifact.digest()),
        }
    }

    pub(crate) fn revalidate_native(&self) -> Result<(), artifact::Error> {
        self.phase.native.revalidate()
    }

    pub(crate) fn take_prepared_read_set(&mut self) -> Option<crate::node::read_set::Prepared> {
        self.phase.read_set.take()
    }

    pub(crate) fn restore_prepared_read_set(
        &mut self,
        read_set: crate::node::read_set::Prepared,
    ) -> Result<(), crate::node::Error> {
        if self.phase.read_set.replace(read_set).is_some() {
            return Err(crate::node::Error::State(
                "built runner already retained a prepared read set",
            ));
        }
        Ok(())
    }

    /// Bind the complete runner closure under the durable Prepared role before
    /// watcher registration or process work.
    pub fn pin_prepared(&self) -> Result<(), crate::node::Error> {
        match self.phase.read_set.as_ref() {
            Some(read_set) => read_set.pin_prepared(),
            None => Ok(()),
        }
    }

    /// Revalidate the complete runner closure after watcher registration and
    /// before process bootstrap.
    pub fn revalidate_prepared(&self) -> Result<(), crate::node::Error> {
        match self.phase.read_set.as_ref() {
            Some(read_set) => read_set.revalidate(),
            None => Ok(()),
        }
    }

    pub(crate) fn pin_live_last_good(&self) -> Result<(), crate::node::Error> {
        match self.phase.read_set.as_ref() {
            Some(read_set) => read_set.pin_live_last_good(),
            None => Ok(()),
        }
    }
}

/// Build one minimal deterministic runner manifest under a supplied test DAG parent.
#[cfg(feature = "test-fixtures")]
#[doc(hidden)]
pub fn fixture(
    chain: crate::node::Chain,
    revision: crate::supervisor::Revision,
) -> Manifest<Generated, ()> {
    use sha2::{Digest as _, Sha256};

    const LOCK: &[u8] = b"# This file is automatically @generated by Cargo.\n# It is not intended for manual editing.\nversion = 4\n\n[[package]]\nname = \"rawr-generated-runner\"\nversion = \"0.0.0\"\n";
    const MANIFEST: &[u8] =
        b"[package]\nname = \"rawr-generated-runner\"\nversion = \"0.0.0\"\nedition = \"2024\"\n";
    const SOURCE: &[u8] = b"fn main() {}\n";

    let key = crate::bridge::BuildRequestKey::fixture([0x52; 32]);
    let records: Box<[manifest::FileRecord]> = Box::from([
        manifest::FileRecord {
            path: "Cargo.lock".into(),
            bytes: LOCK.len(),
            digest: Sha256::digest(LOCK).into(),
        },
        manifest::FileRecord {
            path: "Cargo.toml".into(),
            bytes: MANIFEST.len(),
            digest: Sha256::digest(MANIFEST).into(),
        },
        manifest::FileRecord {
            path: "src/main.rs".into(),
            bytes: SOURCE.len(),
            digest: Sha256::digest(SOURCE).into(),
        },
    ]);
    let files = Box::from([
        manifest::File::new("Cargo.lock", Box::from(LOCK), records[0].digest),
        manifest::File::new("Cargo.toml", Box::from(MANIFEST), records[1].digest),
        manifest::File::new("src/main.rs", Box::from(SOURCE), records[2].digest),
    ]);
    let generated = manifest::GeneratedManifest::new(
        key,
        records,
        Box::default(),
        Box::default(),
        Sha256::digest([]).into(),
    );
    generated
        .validate(&files)
        .expect("the minimal runner fixture manifest validates");
    let limits = crate::cargo::Limits::standard();
    Manifest {
        request: crate::surface::Request::fixture((), &limits),
        native: crate::input::proof::Proof::fixture(),
        profile: crate::profile::Proof::neutral_with_chain(chain),
        revision,
        generated,
        files,
        chain: None,
        overlay: None,
        phase: Generated { _private: () },
    }
}

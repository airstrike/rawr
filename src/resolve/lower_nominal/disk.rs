//! The kept lowerings and readings a runner keeps on disk, under its
//! project's `target/rawr`, so the next runner's start lowers and reads
//! again only the sources whose key changed (#1638).
//!
//! Each source's part has one file, named for the source's path, holding
//! the key it was made at. A file another build of rawr wrote, a file
//! whose key isn't the one asked for, and a file that doesn't decode are
//! each a miss: the part is made again and its file written over.
//!
//! The outline of the image's own facades (`host::image::Outline`) has one
//! file too, at the key of its facades' digests, so a respawned runner of
//! the same image reads it rather than decoding every body again (#1755).

use crate::stored::Known;
use crate::Engine;
use sha2::{Digest as _, Sha256};
use std::path::PathBuf;

const MAGIC: &[u8; 4] = b"RKLW";

/// Bumped when what a file holds changes in a way the build stamp doesn't
/// show.
const FORMAT: u32 = 4;

/// A runner's files of kept lowerings and readings.
pub(crate) struct Disk {
    directory: PathBuf,
    /// The build of rawr that writes and reads them: a file another build
    /// wrote is never decoded.
    stamp: [u8; 32],
    /// The types and facades a file may name, by how many facades were
    /// registered when they were found.
    known: Option<(usize, Known)>,
}

/// What one file holds of a source.
#[derive(Clone, Copy)]
pub(super) enum Kind {
    /// A part's lowering.
    Part(super::kept::Part),
    /// Reveal's readings of its bodies.
    Readings,
    /// The outline of the image's own facades, one for the runner.
    Outline,
}

/// One source's file of one kind, at the key it is made at.
#[derive(Clone, Copy)]
pub(super) struct Entry<'a> {
    /// The source's identity, its path.
    pub(super) identity: &'a str,
    pub(super) kind: Kind,
    pub(super) key: &'a [u8; 32],
}

impl Disk {
    /// Files in `directory`, for this build of rawr. Any runner of the
    /// project with this build reads the files another wrote (#1725): each
    /// file's key holds what its lowering read of the runner's native
    /// surface.
    pub(crate) fn new(directory: PathBuf) -> Self {
        let mut stamp = Sha256::new();
        stamp.update(FORMAT.to_le_bytes());
        stamp.update(env!("CARGO_PKG_VERSION").as_bytes());
        stamp.update(env!("RAWR_BUILD_DIGEST").as_bytes());
        Self {
            directory,
            stamp: stamp.finalize().into(),
            known: None,
        }
    }

    /// What `entry`'s file holds, when this build wrote it at the entry's
    /// key and it decodes.
    pub(super) fn read<T: serde::de::DeserializeOwned>(
        &mut self,
        engine: &Engine,
        entry: Entry<'_>,
    ) -> Option<T> {
        let bytes = std::fs::read(self.path(entry)).ok()?;
        let rest = bytes.strip_prefix(MAGIC.as_slice())?;
        let rest = rest.strip_prefix(self.stamp.as_slice())?;
        let rest = rest.strip_prefix(entry.key.as_slice())?;
        self.known(engine)
            .within(|| rmp_serde::from_slice(rest))
            .ok()
    }

    /// Write `value` as `entry`'s file. A value naming a type no read
    /// would know, or a file that can't be written, is no file.
    pub(super) fn write<T: serde::Serialize>(
        &mut self,
        engine: &Engine,
        entry: Entry<'_>,
        value: &T,
    ) {
        let mut bytes = Vec::from(MAGIC.as_slice());
        bytes.extend_from_slice(&self.stamp);
        bytes.extend_from_slice(entry.key);
        let encoded = self
            .known(engine)
            .within(|| rmp_serde::encode::write(&mut bytes, value));
        let path = self.path(entry);
        if encoded.is_err() {
            // An earlier file of the entry is another key's.
            let _ = std::fs::remove_file(&path);
            return;
        }
        let partial = path.with_extension(format!("{}.partial", std::process::id()));
        let written = std::fs::create_dir_all(&self.directory)
            .and_then(|()| std::fs::write(&partial, &bytes))
            .and_then(|()| std::fs::rename(&partial, &path));
        if written.is_err() {
            let _ = std::fs::remove_file(&partial);
        }
    }

    /// What a file may name: the runtime's own value types and those the
    /// engine's facades link, the engine's facades and its runner's carrier,
    /// found again when a facade was registered since.
    fn known(&mut self, engine: &Engine) -> Known {
        let registered = Engine::registered_facades();
        if let Some((found, known)) = &self.known
            && *found == registered
        {
            return known.clone();
        }
        let known = Known::new(
            super::ty::Ty::native_type_ids().chain(engine.host_registry.linked()),
            engine.host_registry.facades(),
            engine
                .imported_native
                .as_ref()
                .map(|native| *native.carrier().identity()),
        );
        self.known = Some((registered, known.clone()));
        known
    }

    fn path(&self, entry: Entry<'_>) -> PathBuf {
        let digest = Sha256::digest(entry.identity.as_bytes());
        let mut name = String::with_capacity(40);
        for byte in &digest[..16] {
            name.push_str(&format!("{byte:02x}"));
        }
        name.push_str(match entry.kind {
            Kind::Part(super::kept::Part::Bodies) => ".bodies",
            Kind::Part(super::kept::Part::Script) => ".script",
            Kind::Readings => ".readings",
            Kind::Outline => ".outline",
        });
        self.directory.join(name)
    }
}

//! A shared resolution remembered in the project's Rawr directory (D609).
//!
//! A run that every selected crate's shared table answers still asks Cargo
//! how the project resolves and replays the tables: about 3 s for iced in a
//! debug build, every run. The resolution is a function of the request
//! (whose identity covers the manifests, the lock, the config, the target
//! and the demand), the pinned toolchain, the files its proof names, and
//! the shared entries it read. So the next run with all of those unchanged
//! takes the same answer back: it reads those files again, and compares each
//! entry's publication generation with the one captured during replay.
//! Anything else runs the resolution again.
//!
//! What the API is built from is kept too: the sidecar document the tables
//! replayed, its external crates and the API context, all plain data. A
//! consumer that rebuilds the API, which may run under the store's
//! ownership lease, reads those files and nothing else: running the
//! resolution again there would wait on that same lease forever.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::bindings::{Script, Settled};
use crate::input::proof::{Entry, Proof, Role};

const SCHEMA: u32 = 6;
/// Kept beside the harness copies a shared resolution leaves.
const FILE: &str = "shared/remembered.json";
/// The sidecar document the tables replayed, beside it.
const DOCUMENT: &str = "shared/document.json";

/// One file a shared resolution read, as its proof names it: a harness copy
/// or one of the request's inputs, with its length and digest, or none when
/// it was not there.
#[derive(Clone, Deserialize, Serialize)]
pub(super) struct File {
    harness: bool,
    path: String,
    read: Option<(usize, String)>,
}

impl File {
    pub(super) fn new(role: Role, path: &str, read: Option<(usize, [u8; 32])>) -> Self {
        Self {
            harness: role == Role::Harness,
            path: path.to_owned(),
            read: read.map(|(bytes, digest)| (bytes, crate::extract::engine::hex(&digest))),
        }
    }

    fn entry(&self) -> Option<Entry> {
        let role = if self.harness {
            Role::Harness
        } else {
            Role::Authority
        };
        Some(match &self.read {
            Some((bytes, digest)) => Entry::File {
                role,
                package_id: None,
                path: self.path.as_str().into(),
                bytes: *bytes,
                digest: unhex(digest)?,
            },
            None => Entry::Missing {
                role,
                package_id: None,
                path: self.path.as_str().into(),
            },
        })
    }

    /// Whether the file is as the resolution read it. Its digest is the one
    /// this process computed for the file's current identity, if any
    /// (`file_digest`).
    fn current(&self) -> bool {
        match (
            crate::file_digest::sha256(Path::new(&self.path)),
            &self.read,
        ) {
            (Ok((bytes, digest)), Some((length, expected))) => {
                bytes == *length as u64 && crate::extract::engine::hex(&digest) == *expected
            }
            (Err(_), None) => true,
            _ => false,
        }
    }
}

/// One shared entry the resolution read, and the publication it consumed.
#[derive(Deserialize, Serialize)]
struct Used {
    /// Where the table was read: rawr's home, or the project's bindings
    /// for a path closure (#398).
    directory: PathBuf,
    key: String,
    crate_name: String,
    scripts: BTreeMap<String, Vec<Script>>,
    inputs: crate::input::bindings::Inputs,
    generation: String,
}

/// A shared resolution, as the next run takes it back.
#[derive(Deserialize)]
pub(super) struct Memo {
    schema: u32,
    request: String,
    attestation: String,
    identity: String,
    native: String,
    files: Vec<File>,
    entries: Vec<Used>,
    /// Current file and directory proofs produced while validating `entries`.
    #[serde(skip)]
    witnesses: Vec<Proof>,
    /// The document's length and digest.
    document: (u64, String),
    external: Vec<crate::extract::ExternalBinding>,
    context: crate::api::Context,
}

/// A `Memo` as a run writes it, borrowing what the API is built from.
#[derive(Serialize)]
struct Written<'a> {
    schema: u32,
    request: String,
    attestation: String,
    identity: String,
    native: String,
    files: &'a [File],
    entries: Vec<Used>,
    document: (u64, String),
    external: &'a [crate::extract::ExternalBinding],
    context: &'a crate::api::Context,
}

impl Memo {
    /// The identity of the sidecar document the tables replayed.
    pub(super) fn identity(&self) -> Option<[u8; 32]> {
        unhex(&self.identity)
    }

    /// The proof of the files the resolution read, as it gave it.
    pub(super) fn proof(&self) -> Option<Proof> {
        let entries = self
            .files
            .iter()
            .map(File::entry)
            .collect::<Option<Vec<_>>>()?;
        Some(Proof::shared(
            entries.into_boxed_slice(),
            unhex(&self.native)?,
            &self.witnesses,
        ))
    }

    /// The API context the resolution was read under.
    pub(super) const fn context(&self) -> &crate::api::Context {
        &self.context
    }

    /// What the API is built from, read from disk alone.
    pub(super) fn materials(self, rawr: &Path) -> Option<super::Materials> {
        let (document, digest) = crate::file_digest::read_sha256(&rawr.join(DOCUMENT)).ok()?;
        (document.len() as u64 == self.document.0
            && crate::extract::engine::hex(&digest) == self.document.1)
            .then_some(super::Materials {
                document,
                external: self.external,
                context: self.context,
            })
    }

    /// The tables the resolution read, settled once the runner builds.
    pub(super) fn settled(&self) -> Vec<Settled> {
        self.entries
            .iter()
            .map(|used| Settled::Read {
                directory: used.directory.clone(),
                key: used.key.clone(),
                crate_name: used.crate_name.clone(),
                scripts: used.scripts.clone(),
                inputs: used.inputs.clone(),
            })
            .collect()
    }
}

/// What a shared resolution read, to remember for the next run.
pub(super) struct Resolution<'a> {
    pub(super) request: &'a [u8; 32],
    pub(super) attestation: &'a [u8; 32],
    pub(super) identity: &'a [u8; 32],
    pub(super) native: &'a [u8; 32],
    pub(super) files: &'a [File],
    pub(super) settled: &'a [Settled],
    pub(super) generations: &'a [(String, String)],
    pub(super) materials: &'a super::Materials,
}

/// Remember `resolution` under the project's Rawr directory. Best effort:
/// nothing remembered runs the resolution again.
pub(super) fn remember(rawr: &Path, resolution: &Resolution<'_>) {
    let mut entries = Vec::new();
    for settled in resolution.settled {
        let Settled::Read {
            directory,
            key,
            crate_name,
            scripts,
            inputs,
        } = settled
        else {
            return;
        };
        let Some((_, generation)) = resolution.generations.iter().find(|(used, _)| used == key)
        else {
            return;
        };
        // Keep the generation actually replayed, never a later publication
        // observed while recording the memo. Replacement here is a miss.
        if crate::extract::engine::home::generation(directory, key).as_ref() != Some(generation) {
            return;
        }
        entries.push(Used {
            directory: directory.clone(),
            key: key.clone(),
            crate_name: crate_name.clone(),
            scripts: scripts.clone(),
            inputs: inputs.clone(),
            generation: generation.clone(),
        });
    }
    let hex = crate::extract::engine::hex;
    let materials = resolution.materials;
    // The memo is written only after its document, so it never names one
    // that is not there.
    if !write(&rawr.join(DOCUMENT), &materials.document) {
        return;
    }
    let memo = Written {
        schema: SCHEMA,
        request: hex(resolution.request),
        attestation: hex(resolution.attestation),
        identity: hex(resolution.identity),
        native: hex(resolution.native),
        files: resolution.files,
        entries,
        document: (
            materials.document.len() as u64,
            hex(&Sha256::digest(&materials.document)),
        ),
        external: &materials.external,
        context: &materials.context,
    };
    if let Ok(bytes) = serde_json::to_vec(&memo) {
        write(&rawr.join(FILE), &bytes);
    }
}

/// Write `bytes` to `path` whole, through a temporary beside it.
fn write(path: &Path, bytes: &[u8]) -> bool {
    let temporary = path.with_extension(format!("tmp.{}", std::process::id()));
    fs::write(&temporary, bytes).is_ok() && fs::rename(&temporary, path).is_ok()
}

/// The remembered resolution for `request` under `attestation`, when every
/// file it read and every entry it used is as it was. Each entry's use is
/// marked, as reading it would.
pub(super) fn recall(
    rawr: &Path,
    request: &[u8; 32],
    attestation: &[u8; 32],
    current: &crate::input::bindings::Current,
) -> Option<Memo> {
    let mut memo = serde_json::from_slice::<Memo>(&fs::read(rawr.join(FILE)).ok()?).ok()?;
    let hex = crate::extract::engine::hex;
    if memo.schema != SCHEMA
        || memo.request != hex(request)
        || memo.attestation != hex(attestation)
        || memo.entries.is_empty()
        || !memo.files.iter().all(File::current)
    {
        return None;
    }
    for used in &memo.entries {
        let witness = used.witness(current)?;
        if crate::extract::engine::home::generation(&used.directory, &used.key).as_ref()
            != Some(&used.generation)
        {
            return None;
        }
        memo.witnesses.push(witness);
    }
    Some(memo)
}

/// The last validation of each shared entry's inputs in this process, by
/// the entry's directory, key and generation.
static WITNESSED: Mutex<BTreeMap<(PathBuf, String, String), Witnessed>> =
    Mutex::new(BTreeMap::new());

#[cfg(test)]
thread_local! {
    /// Validations this test thread answered from the last one's proof.
    static WITNESS_HITS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// Entries whose validation is remembered; past it, the memo starts over.
const WITNESSED_ENTRIES: usize = 1 << 14;

/// One validation of an entry's inputs under `current`, and the proof of
/// the files it read.
struct Witnessed {
    inputs: crate::input::bindings::Inputs,
    current: crate::input::bindings::Current,
    proof: Proof,
    /// The stamps of every path `proof` read, taken before it read them,
    /// once a validation read the same paths as the one before it.
    watched: Option<super::super::watched::Watched>,
}

impl Used {
    /// The current proof of the files this entry's inputs name, or none when
    /// they don't validate under `current`. While the stamps taken before the
    /// last validation read them all hold, that validation's proof is the
    /// answer, with no file read again.
    fn witness(&self, current: &crate::input::bindings::Current) -> Option<Proof> {
        let key = (
            self.directory.clone(),
            self.key.clone(),
            self.generation.clone(),
        );
        let last = WITNESSED
            .lock()
            .ok()?
            .remove(&key)
            .filter(|last| last.inputs == self.inputs && last.current == *current);
        let unchanged = last.as_ref().is_some_and(|last| {
            last.watched
                .as_ref()
                .is_some_and(super::super::watched::Watched::unchanged)
        });
        if unchanged && let Some(last) = last {
            #[cfg(test)]
            WITNESS_HITS.with(|hits| hits.set(hits.get() + 1));
            let proof = last.proof.clone();
            remember_witness(key, last);
            return Some(proof);
        }
        // What the last validation read, stamped before it is read again.
        let watched = last
            .as_ref()
            .map(|last| super::super::watched::Watched::observe_entries(last.proof.entries()));
        let proof = self.inputs.validate(current)?.into_filesystem();
        let watched = watched.filter(|_| {
            last.as_ref()
                .is_some_and(|last| last.proof.entries() == proof.entries())
        });
        remember_witness(
            key,
            Witnessed {
                inputs: self.inputs.clone(),
                current: current.clone(),
                proof: proof.clone(),
                watched,
            },
        );
        Some(proof)
    }
}

fn remember_witness(key: (PathBuf, String, String), witnessed: Witnessed) {
    if let Ok(mut remembered) = WITNESSED.lock() {
        if remembered.len() >= WITNESSED_ENTRIES {
            remembered.clear();
        }
        remembered.insert(key, witnessed);
    }
}

fn unhex(text: &str) -> Option<[u8; 32]> {
    let mut bytes = [0; 32];
    if text.len() != 64 {
        return None;
    }
    for (index, byte) in bytes.iter_mut().enumerate() {
        *byte = u8::from_str_radix(text.get(index * 2..index * 2 + 2)?, 16).ok()?;
    }
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{recall, remember, File, Resolution};
    use crate::bindings::Settled;
    use crate::input::proof::Role;

    fn scratch() -> PathBuf {
        let root = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../target/tests"))
            .join(format!("rawr-remembered-shared-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("rawr/shared")).unwrap();
        root.canonicalize().unwrap()
    }

    #[test]
    fn a_remembered_resolution_returns_until_a_file_or_an_entry_changes() {
        use sha2::{Digest as _, Sha256};
        let root = scratch();
        let rawr = root.join("rawr");
        let bindings = root.join("bindings");
        crate::extract::engine::home::publish(&bindings, "iced", &[("table.json", b"units")])
            .unwrap();
        let (lease, _) = crate::extract::engine::home::read(&bindings, "iced").unwrap();
        let generations = [("iced".to_owned(), lease.generation().to_owned())];
        drop(lease);
        let manifest = root.join("Cargo.toml");
        std::fs::write(&manifest, "[package]").unwrap();
        let lock = root.join("Cargo.lock");
        let read = |path: &PathBuf| {
            std::fs::read(path)
                .ok()
                .map(|bytes| (bytes.len(), Sha256::digest(&bytes).into()))
        };
        let files = [
            File::new(Role::Authority, manifest.to_str().unwrap(), read(&manifest)),
            File::new(Role::Authority, lock.to_str().unwrap(), read(&lock)),
        ];
        let declared = root.join("declared.txt");
        let missing = root.join("absent-input");
        let tree = root.join("declared-tree");
        std::fs::write(&declared, "first").unwrap();
        std::fs::create_dir(&tree).unwrap();
        let settled = [Settled::Read {
            directory: bindings.clone(),
            key: "iced".to_owned(),
            crate_name: "iced".to_owned(),
            scripts: std::collections::BTreeMap::new(),
            inputs: crate::input::bindings::Inputs::fixture_paths(
                std::slice::from_ref(&declared),
                &[missing.clone(), tree.clone()],
            ),
        }];
        let request = [1; 32];
        let attestation = [2; 32];
        let current = crate::input::bindings::Current::fixture();
        remember(
            &rawr,
            &Resolution {
                request: &request,
                attestation: &attestation,
                identity: &[3; 32],
                native: &[4; 32],
                files: &files,
                settled: &settled,
                generations: &generations,
                materials: &super::super::Materials {
                    document: b"{\"schema\":1}".to_vec(),
                    external: Vec::new(),
                    context: crate::api::Context::fixture(),
                },
            },
        );
        let memo = recall(&rawr, &request, &attestation, &current).unwrap();
        assert_eq!(memo.identity(), Some([3; 32]));
        // The next recall stamps what the first read before reading it
        // again, and the one after reads nothing again while those stamps
        // hold (#1759).
        let hits = || super::WITNESS_HITS.with(std::cell::Cell::get);
        let before = hits();
        assert!(recall(&rawr, &request, &attestation, &current).is_some());
        assert_eq!(hits(), before);
        assert!(recall(&rawr, &request, &attestation, &current).is_some());
        assert_eq!(hits(), before + 1);
        let proof = memo.proof().unwrap();
        let watches = proof.watch().collect::<Vec<_>>();
        assert!(watches.contains(&crate::input::proof::Watch::Exact(declared.clone())));
        assert!(watches.contains(&crate::input::proof::Watch::Exact(missing.clone())));
        assert!(watches.contains(&crate::input::proof::Watch::Tree(tree.clone())));
        assert_eq!(memo.settled().len(), 1);
        // What the API is built from comes back from disk alone, and a
        // document that changed does not.
        let materials = memo.materials(&rawr).unwrap();
        assert_eq!(materials.document, b"{\"schema\":1}");
        std::fs::write(rawr.join(super::DOCUMENT), "{}").unwrap();
        let memo = recall(&rawr, &request, &attestation, &current).unwrap();
        assert!(memo.materials(&rawr).is_none());
        // Declared input changes cannot return the historical API or its
        // historical watch projection, including creation of an absent path.
        std::fs::write(&declared, "second").unwrap();
        assert!(recall(&rawr, &request, &attestation, &current).is_none());
        std::fs::write(&declared, "first").unwrap();
        std::fs::write(&missing, "created").unwrap();
        assert!(recall(&rawr, &request, &attestation, &current).is_none());
        std::fs::remove_file(&missing).unwrap();
        std::fs::write(tree.join("new.txt"), "created").unwrap();
        assert!(recall(&rawr, &request, &attestation, &current).is_none());
        std::fs::remove_file(tree.join("new.txt")).unwrap();
        // Another request, or another toolchain, runs the resolution.
        assert!(recall(&rawr, &[9; 32], &attestation, &current).is_none());
        assert!(recall(&rawr, &request, &[9; 32], &current).is_none());
        // A lock that appears is a change.
        std::fs::write(&lock, "# lock").unwrap();
        assert!(recall(&rawr, &request, &attestation, &current).is_none());
        std::fs::remove_file(&lock).unwrap();
        assert!(recall(&rawr, &request, &attestation, &current).is_some());
        // A republished entry is a change.
        crate::extract::engine::home::publish(&bindings, "iced", &[("table.json", b"other")])
            .unwrap();
        assert!(recall(&rawr, &request, &attestation, &current).is_none());
        // Replacement after replay but before remembering must not attach
        // the new generation to the old document.
        std::fs::remove_file(rawr.join(super::FILE)).unwrap();
        remember(
            &rawr,
            &Resolution {
                request: &request,
                attestation: &attestation,
                identity: &[3; 32],
                native: &[4; 32],
                files: &files,
                settled: &settled,
                generations: &generations,
                materials: &super::super::Materials {
                    document: b"old document".to_vec(),
                    external: Vec::new(),
                    context: crate::api::Context::fixture(),
                },
            },
        );
        assert!(!rawr.join(super::FILE).exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}

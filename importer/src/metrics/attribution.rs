//! Closed title-causality evidence over exact raw cache manifests.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::os::unix::ffi::OsStringExt as _;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use super::cell::project::{self, Manifest, ManifestMember};

const SCHEMA: &str = "rawr.phase9.title-cache-attribution/v5";
const NONCE_DERIVATION: &str = "rawr.extract.plan.run-and-invocation-nonce/v1";
const PLAN_PATH: &[u8] = b"cache/resolution/extraction/active/plan.json";
const PROTOCOL_PREFIX: &[u8] = b"cache/resolution/extraction/active/protocol/";
const CANDIDATE_PREFIX: &[u8] = b"cache/resolution/candidates/";
const PLAN_SUFFIX: &[u8] = b"/extraction/active/plan.json";
const PROTOCOL_SUFFIX: &[u8] = b"/extraction/active/protocol/";
const DOCUMENT_BYTES: u64 = 64 * 1024 * 1024;
const REDACTION: &[u8; 64] = b"0000000000000000000000000000000000000000000000000000000000000000";

/// Reviewed source and executable identity that authorizes one projection.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Authority {
    schema: Box<str>,
    nonce_derivation: Box<str>,
    source_tree: Box<str>,
    executable_digest_sha256: Box<str>,
}

/// One projected member; paths and lengths remain raw and exact.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProjectedMember {
    pub path: Box<[u8]>,
    pub bytes: u64,
    pub digest: [u8; 32],
    pub mode: u32,
}

/// Complete projected manifest used only for title-causality comparison.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct ProjectedManifest {
    pub bytes: u64,
    pub digest: [u8; 32],
    pub members: Box<[ProjectedMember]>,
    pub directories: Box<[project::Directory]>,
}

/// One raw manifest plus its authority-bound causal projection.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Snapshot {
    pub authority: Authority,
    pub raw: Manifest,
    pub projected: ProjectedManifest,
}

/// One complete raw transition and its two causal projections.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Transition {
    pub before: Snapshot,
    pub after: Snapshot,
    pub changes: Box<[project::Change]>,
    pub directory_changes: Box<[project::DirectoryChange]>,
}

/// Final proof for two distinct-title retained executions.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct Comparison {
    schema: Box<str>,
    pub first_title_digest: [u8; 32],
    pub second_title_digest: [u8; 32],
    pub first: Transition,
    pub second: Transition,
}

/// Literal raw-only release equality proof; projections cannot enter this type.
#[derive(Clone, Debug, Eq, PartialEq, Deserialize, Serialize)]
pub struct RawWarm {
    schema: Box<str>,
    pub state: project::State,
}

/// Failure to seal the closed title-attribution proof.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("title-cache attribution authority is invalid")]
    Authority,
    #[error("title-cache attribution document at {path:?} exceeds its closed bound")]
    DocumentBound { path: Box<[u8]> },
    #[error("title-cache attribution document at {path:?} changed during capture")]
    DocumentDrift { path: Box<[u8]> },
    #[error("title-cache attribution plan does not match the reviewed schema")]
    PlanSchema,
    #[error("title-cache attribution protocol does not match the reviewed schema")]
    ProtocolSchema,
    #[error("title-cache attribution has a missing, extra, or renamed protocol member")]
    ProtocolMembers,
    #[error("title-cache attribution nonce bytes are not exact and local")]
    NonceBytes,
    #[error(
        "title-cache attribution transitions do not have the required raw/projection relationship"
    )]
    Comparison,
    #[error("the release raw warm oracle observed a durable cache mutation")]
    RawWarm,
    #[error("title-cache attribution I/O failed at {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error(transparent)]
    Project(#[from] super::cell::Error),
}

impl Authority {
    /// Transform exact reviewed build facts into projection authority.
    pub fn transform(
        source_tree: Box<str>,
        executable_digest_sha256: Box<str>,
    ) -> Result<Self, Error> {
        if source_tree.is_empty()
            || source_tree.len() > 16 * 1024
            || !lower_hex(&executable_digest_sha256, 64)
        {
            return Err(Error::Authority);
        }
        Ok(Self {
            schema: SCHEMA.into(),
            nonce_derivation: NONCE_DERIVATION.into(),
            source_tree,
            executable_digest_sha256,
        })
    }

    fn validate(&self) -> Result<(), Error> {
        if self.schema.as_ref() != SCHEMA
            || self.nonce_derivation.as_ref() != NONCE_DERIVATION
            || self.source_tree.is_empty()
            || self.source_tree.len() > 16 * 1024
            || !lower_hex(&self.executable_digest_sha256, 64)
        {
            return Err(Error::Authority);
        }
        Ok(())
    }
}

impl Snapshot {
    /// Capture every raw member and project only reviewed nonce value bytes.
    pub fn read(root: &Path, authority: Authority) -> Result<Self, Error> {
        let raw = Manifest::read(root)?;
        Self::transform(root, authority, raw)
    }

    /// Project an already-sealed raw manifest without rescanning unrelated members.
    pub fn transform(root: &Path, authority: Authority, raw: Manifest) -> Result<Self, Error> {
        authority.validate()?;
        let members = raw
            .members
            .iter()
            .map(|member| (member.path.as_ref(), member))
            .collect::<BTreeMap<_, _>>();
        let (plan_path, protocol_prefix) = attribution_paths(&raw)?;
        let plan_member = members
            .get(plan_path.as_ref())
            .copied()
            .ok_or(Error::PlanSchema)?;
        let plan_bytes = read_member(root, plan_member)?;
        let entries = crate::extract::plan::attribution_entries(&plan_bytes)
            .map_err(|_| Error::PlanSchema)?;
        let mut expected_protocol = BTreeSet::new();
        let mut arguments_digest: Option<Box<str>> = None;
        let mut projected_digests = BTreeMap::<Box<[u8]>, [u8; 32]>::new();
        let plan_nonces = entries
            .iter()
            .map(|entry| entry.invocation_nonce.as_ref())
            .collect::<Vec<_>>();
        let projected_plan = redact_values(&plan_bytes, &plan_nonces)?;
        projected_digests.insert(plan_path, Sha256::digest(projected_plan).into());
        for entry in &entries {
            let mut path = Vec::with_capacity(protocol_prefix.len().saturating_add(69));
            path.extend_from_slice(&protocol_prefix);
            path.extend_from_slice(entry.unit_correlation.as_bytes());
            path.extend_from_slice(b".json");
            let path = path.into_boxed_slice();
            if !expected_protocol.insert(path.clone()) {
                return Err(Error::ProtocolMembers);
            }
            let member = members
                .get(path.as_ref())
                .copied()
                .ok_or(Error::ProtocolMembers)?;
            let bytes = read_member(root, member)?;
            let (correlation, nonce, digest) =
                crate::extract::protocol::attribution_handshake(&bytes)
                    .map_err(|_| Error::ProtocolSchema)?;
            if correlation != entry.unit_correlation || nonce != entry.invocation_nonce {
                return Err(Error::ProtocolSchema);
            }
            if arguments_digest
                .as_ref()
                .is_some_and(|expected| expected != &digest)
            {
                return Err(Error::ProtocolSchema);
            }
            arguments_digest = Some(digest);
            let projected = redact_values(&bytes, &[nonce.as_ref()])?;
            projected_digests.insert(path.clone(), Sha256::digest(projected).into());
        }
        let mut completion_path = protocol_prefix.to_vec();
        completion_path.extend_from_slice(b"attempts");
        let completion_path = completion_path.into_boxed_slice();
        let member = members
            .get(completion_path.as_ref())
            .copied()
            .ok_or(Error::ProtocolMembers)?;
        if member.bytes > crate::extract::protocol::attempt::MAX_WIRE_BYTES as u64 {
            return Err(Error::DocumentBound {
                path: completion_path,
            });
        }
        let bytes = read_member(root, member)?;
        let completion = crate::extract::protocol::attempt::Completion::decode(&bytes)
            .map_err(|_| Error::ProtocolSchema)?;
        completion
            .require_invocation(
                arguments_digest.as_deref().ok_or(Error::ProtocolSchema)?,
                entries
                    .iter()
                    .map(|entry| crate::extract::protocol::attempt::Invocation {
                        nonce: &entry.invocation_nonce,
                        correlation: &entry.unit_correlation,
                    }),
            )
            .map_err(|_| Error::ProtocolSchema)?;
        let zero = std::str::from_utf8(REDACTION).map_err(|_| Error::NonceBytes)?;
        let invocations = completion
            .invocations()
            .map(|row| crate::extract::protocol::attempt::Invocation {
                nonce: zero,
                correlation: row.correlation,
            })
            .collect::<Vec<_>>();
        let paths = completion.paths().collect::<Vec<_>>();
        let queries = completion.queries().collect::<Vec<_>>();
        let mut projected = Vec::with_capacity(bytes.len());
        crate::extract::protocol::attempt::encode(
            &mut projected,
            completion.arguments_digest,
            completion.conservative,
            &invocations,
            paths.iter().copied(),
            queries.iter().copied(),
        )
        .map_err(|_| Error::ProtocolSchema)?;
        if projected.len() != bytes.len() {
            return Err(Error::ProtocolSchema);
        }
        expected_protocol.insert(completion_path.clone());
        projected_digests.insert(completion_path, Sha256::digest(projected).into());
        let observed_protocol = raw
            .members
            .iter()
            .filter(|member| member.path.starts_with(&protocol_prefix))
            .map(|member| member.path.clone())
            .collect::<BTreeSet<_>>();
        if observed_protocol != expected_protocol {
            return Err(Error::ProtocolMembers);
        }
        let projected_members = raw
            .members
            .iter()
            .map(|member| ProjectedMember {
                path: member.path.clone(),
                bytes: member.bytes,
                mode: member.mode,
                digest: projected_digests
                    .get(member.path.as_ref())
                    .copied()
                    .unwrap_or(member.digest),
            })
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let mut digest = Sha256::new();
        digest.update(b"rawr.phase9.title-cache-projected-manifest/v2");
        digest.update((raw.directories.len() as u64).to_le_bytes());
        for directory in &raw.directories {
            digest.update(b"directory");
            digest.update((directory.path.len() as u64).to_le_bytes());
            digest.update(&directory.path);
            digest.update(directory.mode.to_le_bytes());
        }
        digest.update((projected_members.len() as u64).to_le_bytes());
        for member in &projected_members {
            digest.update(b"file");
            digest.update((member.path.len() as u64).to_le_bytes());
            digest.update(&member.path);
            digest.update(member.mode.to_le_bytes());
            digest.update(member.bytes.to_le_bytes());
            digest.update(member.digest);
        }
        Ok(Self {
            authority,
            projected: ProjectedManifest {
                bytes: raw.state.bytes,
                digest: digest.finalize().into(),
                members: projected_members,
                directories: raw.directories.clone(),
            },
            raw,
        })
    }
}

fn attribution_paths(raw: &Manifest) -> Result<(Box<[u8]>, Box<[u8]>), Error> {
    let mut selected = None;
    for member in &raw.members {
        let path = member.path.as_ref();
        if path == PLAN_PATH {
            if selected
                .replace((PLAN_PATH.into(), PROTOCOL_PREFIX.into()))
                .is_some()
            {
                return Err(Error::PlanSchema);
            }
            continue;
        }
        let Some(request) = path
            .strip_prefix(CANDIDATE_PREFIX)
            .and_then(|path| path.strip_suffix(PLAN_SUFFIX))
        else {
            if path.ends_with(PLAN_SUFFIX) {
                return Err(Error::PlanSchema);
            }
            continue;
        };
        if request.len() != 64
            || !request
                .iter()
                .all(|byte| matches!(byte, b'0'..=b'9' | b'a'..=b'f'))
        {
            return Err(Error::PlanSchema);
        }
        let mut protocol = Vec::with_capacity(
            CANDIDATE_PREFIX
                .len()
                .saturating_add(request.len())
                .saturating_add(PROTOCOL_SUFFIX.len()),
        );
        protocol.extend_from_slice(CANDIDATE_PREFIX);
        protocol.extend_from_slice(request);
        protocol.extend_from_slice(PROTOCOL_SUFFIX);
        if selected
            .replace((member.path.clone(), protocol.into_boxed_slice()))
            .is_some()
        {
            return Err(Error::PlanSchema);
        }
    }
    selected.ok_or(Error::PlanSchema)
}

impl Transition {
    /// Transform two snapshots into one complete raw mutation ledger.
    pub fn transform(before: Snapshot, after: Snapshot) -> Result<Self, Error> {
        if before.authority != after.authority {
            return Err(Error::Authority);
        }
        let raw = project::Transition::transform(before.raw.clone(), after.raw.clone());
        Ok(Self {
            before,
            after,
            changes: raw.changes,
            directory_changes: raw.directory_changes,
        })
    }
}

impl Comparison {
    /// Prove two distinct-title transitions differ only in reviewed nonce bytes.
    pub fn transform(
        first_title_digest: [u8; 32],
        second_title_digest: [u8; 32],
        first: Transition,
        second: Transition,
    ) -> Result<Self, Error> {
        first.before.authority.validate()?;
        if first_title_digest == [0; 32]
            || second_title_digest == [0; 32]
            || first_title_digest == second_title_digest
            || first.before.authority != first.after.authority
            || first.before.authority != second.before.authority
            || first.before.authority != second.after.authority
            || second.before.raw != first.after.raw
            || first.changes.is_empty()
            || second.changes.is_empty()
            || !first.directory_changes.is_empty()
            || !second.directory_changes.is_empty()
            || change_shape(&first.changes) != change_shape(&second.changes)
            || !same_shape(&first.before.raw, &first.after.raw)
            || !same_shape(&first.before.raw, &second.before.raw)
            || !same_shape(&first.before.raw, &second.after.raw)
            || first.before.projected != first.after.projected
            || first.before.projected != second.before.projected
            || first.before.projected != second.after.projected
        {
            return Err(Error::Comparison);
        }
        Ok(Self {
            schema: SCHEMA.into(),
            first_title_digest,
            second_title_digest,
            first,
            second,
        })
    }
}

impl RawWarm {
    /// Transform literal raw equality into the only release warm-cache proof.
    pub fn transform(before: &Manifest, after: &Manifest) -> Result<Self, Error> {
        if before != after {
            return Err(Error::RawWarm);
        }
        Ok(Self {
            schema: "rawr.phase9.raw-warm/v1".into(),
            state: before.state.clone(),
        })
    }
}

fn same_shape(left: &Manifest, right: &Manifest) -> bool {
    left.state.bytes == right.state.bytes
        && left.directories == right.directories
        && left.members.len() == right.members.len()
        && left
            .members
            .iter()
            .zip(&right.members)
            .all(|(left, right)| {
                left.path == right.path && left.bytes == right.bytes && left.mode == right.mode
            })
}

fn change_shape(changes: &[project::Change]) -> Vec<(&[u8], Option<u64>, Option<u64>)> {
    changes
        .iter()
        .map(|change| {
            (
                change.path.as_ref(),
                change.before.as_ref().map(|state| state.bytes),
                change.after.as_ref().map(|state| state.bytes),
            )
        })
        .collect()
}

fn read_member(root: &Path, member: &ManifestMember) -> Result<Box<[u8]>, Error> {
    if member.bytes > DOCUMENT_BYTES {
        return Err(Error::DocumentBound {
            path: member.path.clone(),
        });
    }
    let path = root.join(path_from_bytes(&member.path));
    let bytes = fs::read(&path).map_err(|source| Error::Io {
        path: path.clone(),
        source,
    })?;
    if bytes.len() as u64 != member.bytes
        || <[u8; 32]>::from(Sha256::digest(&bytes)) != member.digest
    {
        return Err(Error::DocumentDrift {
            path: member.path.clone(),
        });
    }
    Ok(bytes.into_boxed_slice())
}

fn path_from_bytes(bytes: &[u8]) -> PathBuf {
    PathBuf::from(std::ffi::OsString::from_vec(bytes.to_vec()))
}

fn redact_values(bytes: &[u8], nonces: &[&str]) -> Result<Box<[u8]>, Error> {
    const PREFIX: &[u8] = b"\"invocation_nonce\":\"";
    if occurrences(bytes, PREFIX) != nonces.len() {
        return Err(Error::NonceBytes);
    }
    let mut projected = bytes.to_vec();
    for nonce in nonces {
        if !lower_hex(nonce, 64) {
            return Err(Error::NonceBytes);
        }
        let mut needle = Vec::with_capacity(PREFIX.len().saturating_add(65));
        needle.extend_from_slice(PREFIX);
        needle.extend_from_slice(nonce.as_bytes());
        needle.push(b'"');
        let positions = positions(&projected, &needle);
        if positions.len() != 1 {
            return Err(Error::NonceBytes);
        }
        let start = positions[0].saturating_add(PREFIX.len());
        projected[start..start.saturating_add(REDACTION.len())].copy_from_slice(REDACTION);
    }
    Ok(projected.into_boxed_slice())
}

fn occurrences(bytes: &[u8], needle: &[u8]) -> usize {
    positions(bytes, needle).len()
}

fn positions(bytes: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() || bytes.len() < needle.len() {
        return Vec::new();
    }
    bytes
        .windows(needle.len())
        .enumerate()
        .filter_map(|(index, window)| (window == needle).then_some(index))
        .collect()
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        && value.bytes().any(|byte| byte != b'0')
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

    use super::{Authority, Comparison, Error, RawWarm, Snapshot, Transition};
    use crate::metrics::cell::project::{Directory, Manifest, ManifestMember, State};

    static NEXT: AtomicU64 = AtomicU64::new(0);

    fn manifest(digest: [u8; 32]) -> Manifest {
        Manifest {
            state: State { bytes: 3, digest },
            members: vec![ManifestMember {
                path: b"member".as_slice().into(),
                bytes: 3,
                digest,
                mode: 0o644,
            }]
            .into_boxed_slice(),
            directories: vec![Directory {
                path: Box::default(),
                mode: 0o755,
            }]
            .into_boxed_slice(),
        }
    }

    #[test]
    fn redaction_changes_only_one_exact_nonce_value() {
        let nonce = "123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0";
        let bytes = format!(r#"{{"invocation_nonce":"{nonce}","other":"same"}}"#);
        let projected = super::redact_values(bytes.as_bytes(), &[nonce]).expect("nonce projects");
        assert_eq!(projected.len(), bytes.len());
        assert!(projected
            .windows(64)
            .any(|window| window == super::REDACTION));
        assert!(projected.ends_with(b"\",\"other\":\"same\"}"));
    }

    #[test]
    fn redaction_rejects_non_nonce_or_duplicate_drift() {
        let nonce = "123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0";
        assert!(matches!(
            super::redact_values(br#"{"other":"same"}"#, &[nonce]),
            Err(Error::NonceBytes)
        ));
        let duplicate = format!(
            r#"{{"invocation_nonce":"{nonce}","nested":{{"invocation_nonce":"{nonce}"}}}}"#
        );
        assert!(matches!(
            super::redact_values(duplicate.as_bytes(), &[nonce]),
            Err(Error::NonceBytes)
        ));
    }

    #[test]
    fn raw_warm_gate_cannot_accept_projected_equality() {
        let first = manifest([1; 32]);
        let second = manifest([2; 32]);
        assert!(matches!(
            RawWarm::transform(&first, &second),
            Err(Error::RawWarm)
        ));
        assert!(RawWarm::transform(&first, &first).is_ok());
    }

    #[test]
    fn authority_rejects_unbound_executable_identity() {
        assert!(Authority::transform("source".into(), "not-a-digest".into()).is_err());
    }

    #[test]
    fn prior_path_schema_cannot_authorize_candidate_projection() {
        let mut authority = authority();
        authority.schema = "rawr.phase9.title-cache-attribution/v1".into();
        assert!(matches!(authority.validate(), Err(Error::Authority)));
    }

    #[test]
    fn exact_schema_projection_accepts_only_nonce_value_drift() {
        let root = fixture_root();
        let authority = authority();
        write_documents(&root, '1', "target");
        let before = Snapshot::read(&root, authority.clone()).expect("initial snapshot seals");
        write_documents(&root, '2', "target");
        let first_after = Snapshot::read(&root, authority.clone()).expect("first snapshot seals");
        let second_before = Snapshot::read(&root, authority.clone()).expect("chain seals");
        write_documents(&root, '3', "target");
        let second_after = Snapshot::read(&root, authority).expect("second snapshot seals");
        let first = Transition::transform(before, first_after).expect("first transition seals");
        let second =
            Transition::transform(second_before, second_after).expect("second transition seals");
        assert!(Comparison::transform([1; 32], [2; 32], first, second).is_ok());
        std::fs::remove_dir_all(root).expect("fixture retires");
    }

    #[test]
    fn nonce_projection_retains_empty_directories_and_permission_changes() {
        use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

        for mutation in ["empty-directory", "file-mode", "root-mode"] {
            let root = fixture_root();
            write_documents(&root, '1', "target");
            let before = Snapshot::read(&root, authority()).unwrap();
            write_documents(&root, '2', "target");
            let middle = Snapshot::read(&root, authority()).unwrap();
            write_documents(&root, '3', "target");
            match mutation {
                "empty-directory" => std::fs::create_dir(root.join("empty")).unwrap(),
                "file-mode" => {
                    let path = protocol_path(&root);
                    let mode = std::fs::metadata(&path).unwrap().mode() & 0o7777;
                    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode ^ 0o100))
                        .unwrap();
                }
                _ => {
                    let mode = std::fs::metadata(&root).unwrap().mode() & 0o7777;
                    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(mode ^ 0o1000))
                        .unwrap();
                }
            }
            let after = Snapshot::read(&root, authority()).unwrap();
            assert_eq!(before.raw.state.bytes, after.raw.state.bytes);
            assert_ne!(middle.projected, after.projected);
            assert!(!super::same_shape(&middle.raw, &after.raw));
            let first = Transition::transform(before, middle.clone()).unwrap();
            let second = Transition::transform(middle, after).unwrap();
            assert!(matches!(
                Comparison::transform([1; 32], [2; 32], first, second),
                Err(Error::Comparison)
            ));
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn completion_is_mandatory_and_bound_to_fragments_and_plan() {
        let root = fixture_root();
        write_documents(&root, '1', "target");
        let path = protocol_path(&root).with_file_name("attempts");
        let original = std::fs::read(&path).expect("completion reads");
        std::fs::remove_file(&path).expect("completion removes");
        assert!(matches!(
            Snapshot::read(&root, authority()),
            Err(Error::ProtocolMembers)
        ));
        std::fs::write(&path, &original[..original.len() - 1]).expect("truncation writes");
        assert!(matches!(
            Snapshot::read(&root, authority()),
            Err(Error::ProtocolSchema)
        ));
        for (digest, nonce, correlation) in [('a', '1', 'c'), ('b', '2', 'c'), ('b', '1', 'd')] {
            let mut bytes = Vec::new();
            crate::extract::protocol::attempt::encode(
                &mut bytes,
                &repeated(digest),
                false,
                &[crate::extract::protocol::attempt::Invocation {
                    nonce: &repeated(nonce),
                    correlation: &repeated(correlation),
                }],
                std::iter::empty(),
                std::iter::empty(),
            )
            .expect("foreign completion encodes");
            std::fs::write(&path, bytes).expect("foreign completion writes");
            assert!(matches!(
                Snapshot::read(&root, authority()),
                Err(Error::ProtocolSchema)
            ));
        }
        std::fs::remove_dir_all(root).expect("fixture retires");
    }

    #[test]
    fn completion_attempt_paths_remain_exact_in_projection() {
        let root = fixture_root();
        write_documents(&root, '1', "target");
        let before = Snapshot::read(&root, authority()).expect("initial snapshot");
        let mut bytes = Vec::new();
        crate::extract::protocol::attempt::encode(
            &mut bytes,
            &repeated('b'),
            false,
            &[crate::extract::protocol::attempt::Invocation {
                nonce: &repeated('1'),
                correlation: &repeated('c'),
            }],
            ["/fixture/attempt.rlib"].into_iter(),
            std::iter::empty(),
        )
        .expect("attempt completion encodes");
        std::fs::write(protocol_path(&root).with_file_name("attempts"), bytes)
            .expect("attempt completion writes");
        let after = Snapshot::read(&root, authority()).expect("changed snapshot");
        assert_ne!(before.projected, after.projected);
        std::fs::remove_dir_all(root).expect("fixture retires");
    }

    #[test]
    fn completion_query_mutation_changes_projection() {
        let root = fixture_root();
        write_documents(&root, '1', "target");
        let mut previous = Snapshot::read(&root, authority()).expect("initial snapshot");
        for queries in [
            ["/fixture/libfirst.rmeta", "/fixture/libsecond.rmeta"],
            ["/fixture/libfirst.rmeta", "/fixture/libthird.rmeta"],
            ["/fixture/libthird.rmeta", "/fixture/libfirst.rmeta"],
        ] {
            let mut bytes = Vec::new();
            crate::extract::protocol::attempt::encode(
                &mut bytes,
                &repeated('b'),
                false,
                &[crate::extract::protocol::attempt::Invocation {
                    nonce: &repeated('1'),
                    correlation: &repeated('c'),
                }],
                std::iter::empty(),
                queries.into_iter(),
            )
            .expect("query completion encodes");
            std::fs::write(protocol_path(&root).with_file_name("attempts"), bytes)
                .expect("query completion writes");
            let current = Snapshot::read(&root, authority()).expect("changed snapshot");
            assert_ne!(previous.projected, current.projected);
            previous = current;
        }
        std::fs::remove_dir_all(root).expect("fixture retires");
    }

    #[test]
    fn schema_projection_rejects_non_nonce_and_path_drift() {
        let root = fixture_root();
        let authority = authority();
        write_documents(&root, '4', "target");
        let before = Snapshot::read(&root, authority.clone()).expect("initial snapshot seals");
        write_documents(&root, '5', "target");
        let first_after = Snapshot::read(&root, authority.clone()).expect("first snapshot seals");
        let second_before = Snapshot::read(&root, authority.clone()).expect("chain seals");
        write_documents(&root, '6', "changed-target");
        let second_after =
            Snapshot::read(&root, authority.clone()).expect("changed snapshot seals");
        let first = Transition::transform(before, first_after).expect("first transition seals");
        let second =
            Transition::transform(second_before, second_after).expect("second transition seals");
        assert!(matches!(
            Comparison::transform([3; 32], [4; 32], first, second),
            Err(Error::Comparison)
        ));

        write_documents(&root, '7', "target");
        let protocol = protocol_path(&root);
        let renamed = protocol.with_file_name(format!("{}.json", repeated('a')));
        std::fs::rename(&protocol, &renamed).expect("protocol renames");
        assert!(matches!(
            Snapshot::read(&root, authority),
            Err(Error::ProtocolMembers)
        ));
        std::fs::remove_dir_all(root).expect("fixture retires");
    }

    #[test]
    fn schema_projection_rejects_malformed_plan() {
        let root = fixture_root();
        write_documents(&root, '8', "target");
        std::fs::write(
            root.join(super::path_from_bytes(super::PLAN_PATH)),
            b"not-json",
        )
        .expect("plan corrupts");
        assert!(matches!(
            Snapshot::read(&root, authority()),
            Err(Error::PlanSchema)
        ));
        std::fs::remove_dir_all(root).expect("fixture retires");
    }

    #[test]
    fn exact_candidate_namespace_is_supported_but_ambiguity_rejects() {
        let root = fixture_root();
        write_documents(&root, '8', "target");
        let request = repeated('e');
        let candidate = root.join(format!("cache/resolution/candidates/{request}/extraction"));
        std::fs::create_dir_all(candidate.parent().expect("candidate has parent"))
            .expect("candidate parent creates");
        std::fs::rename(root.join("cache/resolution/extraction"), &candidate)
            .expect("exact candidate namespace installs");
        Snapshot::read(&root, authority()).expect("exact candidate snapshot seals");

        std::fs::create_dir_all(root.join("cache/resolution/extraction/active/protocol"))
            .expect("ambiguous fixed namespace creates");
        std::fs::copy(
            candidate.join("active/plan.json"),
            root.join("cache/resolution/extraction/active/plan.json"),
        )
        .expect("ambiguous plan copies");
        assert!(matches!(
            Snapshot::read(&root, authority()),
            Err(Error::PlanSchema)
        ));
        std::fs::remove_dir_all(root).expect("fixture retires");
    }

    fn fixture_root() -> std::path::PathBuf {
        let root = std::env::temp_dir().join(format!(
            "rawr-title-attribution-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(root.join("cache/resolution/extraction/active/protocol"))
            .expect("fixture directories create");
        root
    }

    fn authority() -> Authority {
        Authority::transform(
            "reviewed-source@head tracked-diff-sha256=proof".into(),
            repeated('d'),
        )
        .expect("authority seals")
    }

    fn repeated(value: char) -> Box<str> {
        std::iter::repeat_n(value, 64)
            .collect::<String>()
            .into_boxed_str()
    }

    fn protocol_path(root: &std::path::Path) -> std::path::PathBuf {
        root.join(format!(
            "cache/resolution/extraction/active/protocol/{}.json",
            repeated('c')
        ))
    }

    fn write_documents(root: &std::path::Path, nonce: char, target: &str) {
        let nonce = repeated(nonce);
        let correlation = repeated('c');
        let package = serde_json::json!({
            "cargo_id": "package 0.1.0 (path+file:///fixture)",
            "name": "package",
            "version": "0.1.0",
            "source": "path+file:///fixture"
        });
        let artifact = serde_json::json!({
            "crate_name": "package",
            "path": "/fixture/package.rmeta",
            "length": 1,
            "digest": repeated('a'),
            "profile": "debug",
            "fresh": true
        });
        let plan = serde_json::json!({
            "schema": 8,
            "protocol_schema": 21,
            "canonical_items": 1,
            "canonical_bytes": 1,
            "compiler_commit": "compiler",
            "host": "aarch64-apple-darwin",
            "selected_target": target,
            "entries": [{
                "package": package.clone(),
                "root": {"kind": "selected", "aliases": ["package"]},
                "unit_key": "unit",
                "dependencies": [],
                "target": target,
                "target_kind": "lib",
                "target_kinds": ["lib"],
                "crate_types": ["rlib"],
                "crate_name": "package",
                "edition": "2024",
                "profile": "debug",
                "features": [],
                "invocation_nonce": nonce,
                "unit_correlation": correlation,
                "artifact": artifact
            }],
            "dependency_directories": []
        });
        let protocol = serde_json::json!({
            "handshake": {
                "schema": 20,
                "package": package,
                "root": {"kind": "selected", "aliases": ["package"]},
                "target": target,
                "target_kind": "lib",
                "target_kinds": ["lib"],
                "crate_types": ["rlib"],
                "crate_name": "package",
                "edition": "2024",
                "profile": "debug",
                "compilation": "target",
                "host": "aarch64-apple-darwin",
                "selected_target": target,
                "features": [],
                "probe_arguments_digest": repeated('b'),
                "compiler_commit": "compiler",
                "invocation_nonce": nonce,
                "unit_correlation": correlation,
                "unit_key": "unit",
                "dependencies": [],
                "artifact": {
                    "path": "/fixture/package.rmeta",
                    "length": 1,
                    "digest": repeated('a'),
                    "profile": "debug",
                    "fresh": true,
                    "crate_hash": "crate-hash",
                    "stable_crate_id": "123456789abcdef0",
                    "probe_alias": "package"
                },
                "sysroot": [],
                "loaded_crates": [],
                "external_crates": []
            },
            "reachable": [],
            "definitions": [],
            "implementations": [],
            "scalar_predicate_proofs": []
        });
        let plan_path = root.join(super::path_from_bytes(super::PLAN_PATH));
        std::fs::write(
            plan_path,
            serde_json::to_vec(&plan).expect("plan serializes"),
        )
        .expect("plan writes");
        std::fs::write(
            protocol_path(root),
            serde_json::to_vec(&protocol).expect("protocol serializes"),
        )
        .expect("protocol writes");
        let mut completion = Vec::new();
        crate::extract::protocol::attempt::encode(
            &mut completion,
            &repeated('b'),
            false,
            &[crate::extract::protocol::attempt::Invocation {
                nonce: &nonce,
                correlation: &correlation,
            }],
            std::iter::empty(),
            std::iter::empty(),
        )
        .expect("completion encodes");
        std::fs::write(protocol_path(root).with_file_name("attempts"), completion)
            .expect("completion writes");
        std::fs::write(root.join("stable-member"), b"stable").expect("stable member writes");
    }
}

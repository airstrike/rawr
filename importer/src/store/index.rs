use std::fs;
use std::path::{Path, PathBuf};

use crate::node::{ActionKey, Error, Kind, LookupKey};

const MAGIC: &[u8; 8] = b"RAWRINDX";
const SCHEMA: u16 = 2;
const ROW_BYTES: usize = 112;
const MAX_CANDIDATES: usize = 8;
const MAX_BUCKETS: usize = 65_536;
const MAX_REFERENCES: usize = 524_288;
const MAX_BYTES: usize = 256 * 1024 * 1024;
const MAX_PROJECT_BUCKETS: usize = 4_096;
const MAX_PROJECT_REFERENCES: usize = 32_768;
const MAX_PROJECT_BYTES: usize = 8 * 1024 * 1024;

struct Inventory {
    project: crate::node::Digest,
    buckets: usize,
    references: usize,
    bytes: usize,
    requested: usize,
    project_buckets: usize,
    project_references: usize,
    project_bytes: usize,
    requested_project: bool,
}

pub(crate) struct Candidates {
    pub(crate) actions: Box<[ActionKey]>,
    pub(crate) rejected: bool,
}

pub(crate) fn lookup(
    root: &Path,
    kind: Kind,
    output_schema: u32,
    lookup: LookupKey,
    budget: &crate::node::GraphBudget,
) -> Result<Candidates, Error> {
    let relative = bucket_relative(kind, lookup);
    let bucket = match super::entry::existing_directory_budgeted(root, &relative, budget) {
        Ok(Some(bucket)) => bucket,
        Ok(None) => return Ok(empty(false)),
        Err(error @ Error::Io { .. }) => return Err(error),
        Err(_) => return Ok(empty(true)),
    };
    if !budget_admitted(budget.lookup())? {
        return Ok(empty(true));
    }
    if !budget_admitted(budget.paths(1))? {
        return Ok(empty(true));
    }
    let mut directory = fs::read_dir(&bucket).map_err(|source| Error::Io {
        operation: "read discoverable candidate bucket",
        path: bucket.clone(),
        source,
    })?;
    let mut actions = Vec::new();
    actions
        .try_reserve_exact(MAX_CANDIDATES)
        .map_err(|_| Error::Allocation)?;
    loop {
        if !budget_admitted(budget.paths(1))? {
            return Ok(empty(true));
        }
        let Some(child) = directory.next() else {
            break;
        };
        if actions.len() == MAX_CANDIDATES {
            return Ok(empty(true));
        }
        let child = match child {
            Ok(child) => child,
            Err(_) => return Ok(empty(true)),
        };
        if !budget_admitted(budget.paths(1))? || !budget_admitted(budget.comparison())? {
            return Ok(empty(true));
        }
        let file_type = match child.file_type() {
            Ok(file_type) => file_type,
            Err(_) => return Ok(empty(true)),
        };
        if file_type.is_symlink() || !file_type.is_dir() {
            return Ok(empty(true));
        }
        let row_path = child.path().join("row");
        let bytes = match super::entry::read_file_budgeted(&row_path, ROW_BYTES, budget) {
            Ok(bytes) => bytes,
            Err(_) => return Ok(empty(true)),
        };
        let action = match decode(&bytes, kind, output_schema, lookup) {
            Ok(action) => action,
            Err(_) => return Ok(empty(true)),
        };
        if child.file_name().as_os_str().as_encoded_bytes() != action.directory().as_bytes() {
            return Ok(empty(true));
        }
        actions.push(action);
    }
    actions.sort_unstable();
    actions.dedup();
    Ok(Candidates {
        actions: actions.into_boxed_slice(),
        rejected: false,
    })
}

pub(crate) fn publish(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    kind: Kind,
    output_schema: u32,
    lookup: LookupKey,
    action: ActionKey,
    project: crate::node::Digest,
) -> Result<(), Error> {
    let bucket_relative = bucket_relative(kind, lookup);
    let target_relative = bucket_relative.join(action.directory());
    if let Some(target) = super::entry::existing_directory(root, &target_relative)? {
        let row_path = target.join("row");
        let existing = super::entry::read_file(&row_path, ROW_BYTES)?;
        return if matches!(
            decode(&existing, kind, output_schema, lookup),
            Ok(existing) if existing == action
        ) {
            Ok(())
        } else {
            Err(Error::Corrupt {
                path: row_path,
                reason: "discoverable candidate row changed",
            })
        };
    }
    Inventory::read(root, kind, lookup, project)?.admit(root, kind, lookup)?;
    publish_row(
        publication,
        root,
        kind,
        output_schema,
        lookup,
        action,
        project,
    )
}

/// Replace one already-present, nonempty, bounded advisory bucket after its
/// lookup was rejected. Removing at least one old row before publishing the
/// replacement cannot increase installation-wide bucket or reference usage.
pub(crate) fn repair_existing(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    kind: Kind,
    output_schema: u32,
    lookup: LookupKey,
    action: ActionKey,
    project: crate::node::Digest,
) -> Result<bool, Error> {
    let relative = bucket_relative(kind, lookup);
    let Some(bucket) = super::entry::existing_directory(root, &relative)? else {
        return Ok(false);
    };
    let mut rows = Vec::new();
    rows.try_reserve_exact(MAX_CANDIDATES)
        .map_err(|_| Error::Allocation)?;
    for child in fs::read_dir(&bucket).map_err(|source| Error::Io {
        operation: "read rejected discoverable candidate bucket",
        path: bucket.clone(),
        source,
    })? {
        if rows.len() == MAX_CANDIDATES {
            return Err(Error::CleanupRequired(
                "a rejected candidate bucket exceeded bounded repair",
            ));
        }
        let child = child.map_err(|source| Error::Io {
            operation: "read rejected discoverable candidate row",
            path: bucket.clone(),
            source,
        })?;
        if decode_digest(&child.file_name()).is_none() {
            return Err(Error::CleanupRequired(
                "a rejected candidate row changed canonical placement",
            ));
        }
        let file_type = child.file_type().map_err(|source| Error::Io {
            operation: "inspect rejected discoverable candidate row",
            path: child.path(),
            source,
        })?;
        if file_type.is_symlink() || !file_type.is_dir() {
            return Err(Error::Shape(child.path()));
        }
        validate_repair_row(&child.path())?;
        rows.push(child.path());
    }
    if rows.is_empty() {
        return Ok(false);
    }
    Inventory::read_replacing(root, kind, lookup, project)?.admit(root, kind, lookup)?;
    fs::remove_dir_all(&bucket).map_err(|source| Error::Io {
        operation: "remove rejected discoverable candidate bucket",
        path: bucket.clone(),
        source,
    })?;
    super::entry::sync_directory(
        bucket
            .parent()
            .ok_or(Error::State("candidate bucket has no kind directory"))?,
    )?;
    publish_row(
        publication,
        root,
        kind,
        output_schema,
        lookup,
        action,
        project,
    )?;
    Ok(true)
}

fn publish_row(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    kind: Kind,
    output_schema: u32,
    lookup: LookupKey,
    action: ActionKey,
    project: crate::node::Digest,
) -> Result<(), Error> {
    let bucket =
        super::entry::ensure_admitted_directory(publication, root, &bucket_relative(kind, lookup))?;
    let target = bucket.join(action.directory());
    let bytes = encode(kind, output_schema, lookup, action, project);
    match super::entry::publish_directory(publication, root, &target, "row", &bytes)? {
        super::entry::Publication::Published => Ok(()),
        super::entry::Publication::Existing => {
            let row_path = target.join("row");
            let existing = super::entry::read_file(&row_path, ROW_BYTES)?;
            if matches!(
                decode(&existing, kind, output_schema, lookup),
                Ok(existing) if existing == action
            ) {
                Ok(())
            } else {
                Err(Error::Corrupt {
                    path: row_path,
                    reason: "discoverable candidate row changed",
                })
            }
        }
    }
}

fn validate_repair_row(root: &Path) -> Result<(), Error> {
    let mut members = fs::read_dir(root).map_err(|source| Error::Io {
        operation: "read rejected discoverable candidate row",
        path: root.to_path_buf(),
        source,
    })?;
    let Some(member) = members.next().transpose().map_err(|source| Error::Io {
        operation: "read rejected discoverable candidate row member",
        path: root.to_path_buf(),
        source,
    })?
    else {
        return Err(Error::CleanupRequired("a rejected candidate row is empty"));
    };
    if member.file_name() != "row" || members.next().is_some() {
        return Err(Error::CleanupRequired(
            "a rejected candidate row changed shape",
        ));
    }
    let metadata = fs::symlink_metadata(member.path()).map_err(|source| Error::Io {
        operation: "inspect rejected discoverable candidate row member",
        path: member.path(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || hard_linked(&metadata) {
        return Err(Error::Shape(member.path()));
    }
    let _ = super::entry::read_file(&member.path(), ROW_BYTES)?;
    Ok(())
}

impl Inventory {
    fn read(
        root: &Path,
        requested_kind: Kind,
        requested_lookup: LookupKey,
        project: crate::node::Digest,
    ) -> Result<Self, Error> {
        Self::read_mode(
            root,
            Some((requested_kind, requested_lookup)),
            project,
            false,
            false,
        )
    }

    fn read_replacing(
        root: &Path,
        requested_kind: Kind,
        requested_lookup: LookupKey,
        project: crate::node::Digest,
    ) -> Result<Self, Error> {
        Self::read_mode(
            root,
            Some((requested_kind, requested_lookup)),
            project,
            true,
            false,
        )
    }

    fn read_mode(
        root: &Path,
        requested: Option<(Kind, LookupKey)>,
        project: crate::node::Digest,
        replacing_requested: bool,
        unowned_on_decode_failure: bool,
    ) -> Result<Self, Error> {
        let indexes = root.join("indexes");
        let mut inventory = Self {
            project,
            buckets: 0,
            references: 0,
            bytes: 0,
            requested: 0,
            project_buckets: 0,
            project_references: 0,
            project_bytes: 0,
            requested_project: false,
        };
        let kinds = fs::read_dir(&indexes).map_err(|source| Error::Io {
            operation: "read typed store candidate indexes",
            path: indexes.clone(),
            source,
        })?;
        for kind in kinds {
            let kind = kind.map_err(|source| Error::Io {
                operation: "read typed store candidate kind",
                path: indexes.clone(),
                source,
            })?;
            let observed_kind = kind_from_directory(&kind.file_name()).ok_or(
                Error::CleanupRequired("a candidate index kind changed canonical placement"),
            )?;
            let file_type = kind.file_type().map_err(|source| Error::Io {
                operation: "inspect typed store candidate kind",
                path: kind.path(),
                source,
            })?;
            if file_type.is_symlink() || !file_type.is_dir() {
                return Err(Error::Shape(kind.path()));
            }
            let buckets = fs::read_dir(kind.path()).map_err(|source| Error::Io {
                operation: "read typed store candidate kind buckets",
                path: kind.path(),
                source,
            })?;
            for bucket in buckets {
                let bucket = bucket.map_err(|source| Error::Io {
                    operation: "read typed store candidate bucket",
                    path: kind.path(),
                    source,
                })?;
                inventory.buckets = inventory.buckets.checked_add(1).ok_or(Error::Allocation)?;
                let lookup = decode_digest(&bucket.file_name())
                    .map(LookupKey::from_bytes)
                    .ok_or(Error::CleanupRequired(
                        "a candidate bucket changed canonical placement",
                    ))?;
                let file_type = bucket.file_type().map_err(|source| Error::Io {
                    operation: "inspect typed store candidate bucket",
                    path: bucket.path(),
                    source,
                })?;
                if file_type.is_symlink() || !file_type.is_dir() {
                    return Err(Error::Shape(bucket.path()));
                }
                let rows = fs::read_dir(bucket.path()).map_err(|source| Error::Io {
                    operation: "read typed store candidate rows",
                    path: bucket.path(),
                    source,
                })?;
                let mut bucket_references = 0usize;
                let mut project_bucket = false;
                let requested_bucket = requested == Some((observed_kind, lookup));
                for row in rows {
                    let row = row.map_err(|source| Error::Io {
                        operation: "read typed store candidate row",
                        path: bucket.path(),
                        source,
                    })?;
                    bucket_references =
                        bucket_references.checked_add(1).ok_or(Error::Allocation)?;
                    let action = decode_digest(&row.file_name())
                        .map(ActionKey::from_bytes)
                        .ok_or(Error::CleanupRequired(
                            "a candidate row changed canonical placement",
                        ))?;
                    let file_type = row.file_type().map_err(|source| Error::Io {
                        operation: "inspect typed store candidate row",
                        path: row.path(),
                        source,
                    })?;
                    if file_type.is_symlink() || !file_type.is_dir() {
                        return Err(Error::Shape(row.path()));
                    }
                    if replacing_requested && requested_bucket {
                        validate_repair_row(&row.path())?;
                        continue;
                    }
                    inventory.references = inventory
                        .references
                        .checked_add(1)
                        .ok_or(Error::Allocation)?;
                    let row_path = row.path().join("row");
                    let bytes = super::entry::read_file(&row_path, ROW_BYTES)?;
                    let decoded = match decode_any(&bytes, observed_kind, lookup) {
                        Ok(decoded) => decoded,
                        Err(_) if unowned_on_decode_failure => continue,
                        Err(error) => return Err(error),
                    };
                    if decoded.action != action {
                        if unowned_on_decode_failure {
                            continue;
                        }
                        return Err(Error::Corrupt {
                            path: row_path,
                            reason: "candidate index row identity changed",
                        });
                    }
                    inventory.bytes = inventory
                        .bytes
                        .checked_add(bytes.len())
                        .ok_or(Error::Allocation)?;
                    if decoded.project == project {
                        project_bucket = true;
                        inventory.project_references = inventory
                            .project_references
                            .checked_add(1)
                            .ok_or(Error::Allocation)?;
                        inventory.project_bytes = inventory
                            .project_bytes
                            .checked_add(bytes.len())
                            .ok_or(Error::Allocation)?;
                    }
                }
                if bucket_references > MAX_CANDIDATES {
                    return Err(Error::CleanupRequired(
                        "a candidate bucket exceeded its bounded cardinality",
                    ));
                }
                if requested_bucket && !replacing_requested {
                    inventory.requested = bucket_references;
                    inventory.requested_project = project_bucket;
                }
                if project_bucket {
                    inventory.project_buckets = inventory
                        .project_buckets
                        .checked_add(1)
                        .ok_or(Error::Allocation)?;
                }
            }
        }
        inventory.enforce()?;
        Ok(inventory)
    }

    fn admit(
        self,
        root: &Path,
        requested_kind: Kind,
        requested_lookup: LookupKey,
    ) -> Result<(), Error> {
        let bucket_exists = super::entry::existing_directory(
            root,
            &PathBuf::from("indexes")
                .join(requested_kind.directory())
                .join(requested_lookup.directory()),
        )?
        .is_some();
        let next = Self {
            project: self.project,
            buckets: self
                .buckets
                .checked_add(usize::from(!bucket_exists))
                .ok_or(Error::Allocation)?,
            references: self.references.checked_add(1).ok_or(Error::Allocation)?,
            bytes: self.bytes.checked_add(ROW_BYTES).ok_or(Error::Allocation)?,
            requested: self.requested.checked_add(1).ok_or(Error::Allocation)?,
            project_buckets: self
                .project_buckets
                .checked_add(usize::from(!self.requested_project))
                .ok_or(Error::Allocation)?,
            project_references: self
                .project_references
                .checked_add(1)
                .ok_or(Error::Allocation)?,
            project_bytes: self
                .project_bytes
                .checked_add(ROW_BYTES)
                .ok_or(Error::Allocation)?,
            requested_project: true,
        };
        next.enforce()
    }

    fn enforce(&self) -> Result<(), Error> {
        for (resource, actual, limit) in [
            (
                "typed store candidate index buckets",
                self.buckets,
                MAX_BUCKETS,
            ),
            (
                "typed store candidate index references",
                self.references,
                MAX_REFERENCES,
            ),
            ("typed store candidate index bytes", self.bytes, MAX_BYTES),
            (
                "typed store candidate bucket references",
                self.requested,
                MAX_CANDIDATES,
            ),
            (
                "typed store project candidate index buckets",
                self.project_buckets,
                MAX_PROJECT_BUCKETS,
            ),
            (
                "typed store project candidate index references",
                self.project_references,
                MAX_PROJECT_REFERENCES,
            ),
            (
                "typed store project candidate index bytes",
                self.project_bytes,
                MAX_PROJECT_BYTES,
            ),
        ] {
            if actual > limit {
                return Err(Error::Limit {
                    resource,
                    limit,
                    actual,
                });
            }
        }
        Ok(())
    }

    fn project_charge(&self) -> Result<super::accounting::Charge, Error> {
        let references = u64::try_from(self.project_references).map_err(|_| Error::Allocation)?;
        let buckets = u64::try_from(self.project_buckets).map_err(|_| Error::Allocation)?;
        let members = references
            .checked_mul(2)
            .and_then(|members| members.checked_add(buckets))
            .ok_or(Error::Allocation)?;
        let bytes = members
            .checked_mul(super::control::directory_charge().bytes)
            .ok_or(Error::Allocation)?;
        Ok(super::accounting::Charge {
            bytes,
            allocations: 0,
            members,
        })
    }
}

pub(super) fn project_charge(
    root: &Path,
    project: crate::node::Digest,
) -> Result<super::accounting::Charge, Error> {
    Inventory::read_mode(root, None, project, false, true)?.project_charge()
}

pub(crate) fn retire_action(
    _publication: &super::lease::Publication<'_>,
    root: &Path,
    kind: Kind,
    action: ActionKey,
) -> Result<(), Error> {
    let relative = PathBuf::from("indexes").join(kind.directory());
    let Some(kind_root) = super::entry::existing_directory(root, &relative)? else {
        return Ok(());
    };
    let mut buckets = fs::read_dir(&kind_root).map_err(|source| Error::Io {
        operation: "read candidate index during action retirement",
        path: kind_root.clone(),
        source,
    })?;
    let mut observed = 0usize;
    for bucket in &mut buckets {
        let bucket = bucket.map_err(|source| Error::Io {
            operation: "read candidate bucket during action retirement",
            path: kind_root.clone(),
            source,
        })?;
        observed = observed.checked_add(1).ok_or(Error::Allocation)?;
        if observed > MAX_BUCKETS {
            return Err(Error::Limit {
                resource: "typed store candidate index buckets",
                limit: MAX_BUCKETS,
                actual: observed,
            });
        }
        if !lower_digest(&bucket.file_name()) {
            return Err(Error::CleanupRequired(
                "a candidate bucket changed canonical placement",
            ));
        }
        let file_type = bucket.file_type().map_err(|source| Error::Io {
            operation: "inspect candidate bucket during action retirement",
            path: bucket.path(),
            source,
        })?;
        if file_type.is_symlink() || !file_type.is_dir() {
            return Err(Error::Shape(bucket.path()));
        }
        let target = bucket.path().join(action.directory());
        match fs::symlink_metadata(&target) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(Error::Shape(target));
            }
            Ok(_) => {
                fs::remove_dir_all(&target).map_err(|source| Error::Io {
                    operation: "remove retired candidate index row",
                    path: target.clone(),
                    source,
                })?;
                super::entry::sync_directory(&bucket.path())?;
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
            Err(source) => {
                return Err(Error::Io {
                    operation: "inspect retired candidate index row",
                    path: target,
                    source,
                });
            }
        }
    }
    Ok(())
}

pub(crate) fn bucket_relative(kind: Kind, lookup: LookupKey) -> PathBuf {
    PathBuf::from("indexes")
        .join(kind.directory())
        .join(lookup.directory())
}

fn encode(
    kind: Kind,
    output_schema: u32,
    lookup: LookupKey,
    action: ActionKey,
    project: crate::node::Digest,
) -> [u8; ROW_BYTES] {
    let mut bytes = [0_u8; ROW_BYTES];
    bytes[..8].copy_from_slice(MAGIC);
    bytes[8..10].copy_from_slice(&SCHEMA.to_le_bytes());
    bytes[10] = kind as u8;
    bytes[12..16].copy_from_slice(&output_schema.to_le_bytes());
    bytes[16..48].copy_from_slice(&lookup.as_bytes());
    bytes[48..80].copy_from_slice(&action.as_bytes());
    bytes[80..112].copy_from_slice(&project.as_bytes());
    bytes
}

struct Decoded {
    action: ActionKey,
    project: crate::node::Digest,
}

fn decode(
    bytes: &[u8],
    kind: Kind,
    output_schema: u32,
    lookup: LookupKey,
) -> Result<ActionKey, Error> {
    let action = decode_any(bytes, kind, lookup)?.action;
    if u32::from_le_bytes(
        bytes[12..16]
            .try_into()
            .map_err(|_| Error::Codec("discoverable candidate schema is truncated"))?,
    ) != output_schema
    {
        return Err(Error::Codec("discoverable candidate row changed"));
    }
    Ok(action)
}

fn decode_any(bytes: &[u8], kind: Kind, lookup: LookupKey) -> Result<Decoded, Error> {
    let schema = bytes
        .get(8..10)
        .and_then(|bytes| bytes.try_into().ok())
        .map(u16::from_le_bytes)
        .ok_or(Error::Codec("discoverable candidate row is truncated"))?;
    if (schema, bytes.len()) != (SCHEMA, ROW_BYTES)
        || &bytes[..8] != MAGIC
        || bytes[10] != kind as u8
        || bytes[11] != 0
        || bytes[16..48] != lookup.as_bytes()
    {
        return Err(Error::Codec("discoverable candidate row changed"));
    }
    let action = bytes[48..80]
        .try_into()
        .map(ActionKey::from_bytes)
        .map_err(|_| Error::Codec("discoverable candidate action is truncated"))?;
    let project = crate::node::Digest::from_bytes(
        bytes[80..112]
            .try_into()
            .map_err(|_| Error::Codec("discoverable candidate project is truncated"))?,
    );
    Ok(Decoded { action, project })
}

fn empty(rejected: bool) -> Candidates {
    Candidates {
        actions: Box::new([]),
        rejected,
    }
}

fn budget_admitted(admission: Result<(), Error>) -> Result<bool, Error> {
    match admission {
        Ok(()) => Ok(true),
        Err(Error::Limit { .. }) => Ok(false),
        Err(error) => Err(error),
    }
}

fn lower_digest(name: &std::ffi::OsStr) -> bool {
    decode_digest(name).is_some()
}

#[cfg(unix)]
fn hard_linked(metadata: &fs::Metadata) -> bool {
    use std::os::unix::fs::MetadataExt as _;

    metadata.nlink() != 1
}

#[cfg(not(unix))]
fn hard_linked(_metadata: &fs::Metadata) -> bool {
    false
}

fn decode_digest(name: &std::ffi::OsStr) -> Option<[u8; 32]> {
    let encoded = name.as_encoded_bytes();
    if encoded.len() != 64 {
        return None;
    }
    let mut decoded = [0; 32];
    for (index, pair) in encoded.as_chunks::<2>().0.iter().enumerate() {
        decoded[index] = decode_nibble(pair[0])?.checked_mul(16)? + decode_nibble(pair[1])?;
    }
    Some(decoded)
}

fn decode_nibble(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        _ => None,
    }
}

fn kind_from_directory(name: &std::ffi::OsStr) -> Option<Kind> {
    match name.as_encoded_bytes() {
        b"toolchain" => Some(Kind::Toolchain),
        b"resolution" => Some(Kind::Resolution),
        b"api" => Some(Kind::Api),
        b"generic" => Some(Kind::Generic),
        b"profile" => Some(Kind::Profile),
        b"facade" => Some(Kind::Facade),
        b"checked" => Some(Kind::Checked),
        b"profile-proof" => Some(Kind::ProfileProof),
        b"native-product" => Some(Kind::NativeProduct),
        b"overlay" => Some(Kind::Overlay),
        b"runner" => Some(Kind::Runner),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture_root(label: &str) -> PathBuf {
        let root =
            std::env::temp_dir().join(format!("rawr-store-index-{label}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir(&root).expect("the index fixture root exists");
        let _store =
            crate::store::Store::open(&root, [0x92; 32]).expect("the index fixture store opens");
        fs::canonicalize(root).expect("the index fixture canonicalizes")
    }

    fn publish_fixture(
        root: &Path,
        kind: Kind,
        schema: u32,
        lookup: LookupKey,
        action: ActionKey,
    ) -> Result<(), Error> {
        super::super::lease::with_publication(root, |publication| {
            publish(
                publication,
                root,
                kind,
                schema,
                lookup,
                action,
                crate::node::Digest::from_bytes([0x92; 32]),
            )
        })
    }

    #[test]
    fn candidate_bucket_admits_exact_cap_then_requires_retirement_before_growth() {
        let root = fixture_root("bucket-cap");
        let key = LookupKey::from_bytes([0x41; 32]);
        let mut actions = Vec::new();
        for index in 0..MAX_CANDIDATES {
            let mut bytes = [0; 32];
            bytes[24..].copy_from_slice(&(index as u64).to_be_bytes());
            let action = ActionKey::from_bytes(bytes);
            publish_fixture(&root, Kind::Runner, 2, key, action)
                .expect("the exact candidate bucket cap admits");
            actions.push(action);
        }
        assert_eq!(
            lookup(
                &root,
                Kind::Runner,
                2,
                key,
                &crate::node::GraphBudget::new(),
            )
            .expect("the exact candidate bucket reads")
            .actions
            .len(),
            MAX_CANDIDATES
        );
        let ninth = ActionKey::from_bytes([0x51; 32]);
        assert!(matches!(
            publish_fixture(&root, Kind::Runner, 2, key, ninth),
            Err(Error::Limit {
                resource: "typed store candidate bucket references",
                limit: MAX_CANDIDATES,
                actual,
            }) if actual == MAX_CANDIDATES + 1
        ));
        assert!(!root
            .join(bucket_relative(Kind::Runner, key))
            .join(ninth.directory())
            .exists());

        super::super::lease::with_publication(&root, |publication| {
            retire_action(publication, &root, Kind::Runner, actions[0])
        })
        .expect("one retired action releases its exact index row");
        publish_fixture(&root, Kind::Runner, 2, key, ninth)
            .expect("one candidate can grow after exact retirement");
        let observed = lookup(
            &root,
            Kind::Runner,
            2,
            key,
            &crate::node::GraphBudget::new(),
        )
        .expect("the replacement candidate bucket reads");
        assert_eq!(observed.actions.len(), MAX_CANDIDATES);
        assert!(observed.actions.binary_search(&actions[0]).is_err());
        assert!(observed.actions.binary_search(&ninth).is_ok());

        fs::remove_dir_all(root).expect("the index fixture retires");
    }

    #[test]
    fn malformed_global_index_rejects_growth_without_mutation() {
        let root = fixture_root("malformed-global");
        fs::create_dir(root.join("indexes/unknown")).expect("the malformed index kind exists");
        let key = LookupKey::from_bytes([0x61; 32]);
        let action = ActionKey::from_bytes([0x62; 32]);

        assert!(matches!(
            publish_fixture(&root, Kind::Runner, 2, key, action),
            Err(Error::CleanupRequired(
                "a candidate index kind changed canonical placement"
            ))
        ));
        assert!(!root.join("indexes/runner").exists());
        fs::remove_dir_all(root).expect("the malformed index fixture retires");
    }

    #[test]
    fn candidate_rows_bind_the_publishing_project() {
        let kind = Kind::Runner;
        let schema = 2;
        let lookup = LookupKey::from_bytes([0x71; 32]);
        let action = ActionKey::from_bytes([0x72; 32]);
        let project = crate::node::Digest::from_bytes([0x73; 32]);
        let current = encode(kind, schema, lookup, action, project);
        let decoded = decode_any(&current, kind, lookup).expect("the current row decodes");
        assert_eq!(decoded.action, action);
        assert_eq!(decoded.project, project);
    }

    #[test]
    fn project_candidate_inventory_enforces_each_exact_cap_and_one_unit_floor() {
        let exact = Inventory {
            project: crate::node::Digest::from_bytes([0x74; 32]),
            buckets: MAX_PROJECT_BUCKETS,
            references: MAX_PROJECT_REFERENCES,
            bytes: MAX_PROJECT_BYTES,
            requested: 0,
            project_buckets: MAX_PROJECT_BUCKETS,
            project_references: MAX_PROJECT_REFERENCES,
            project_bytes: MAX_PROJECT_BYTES,
            requested_project: false,
        };
        exact.enforce().expect("all exact project index caps admit");
        assert_eq!(
            exact
                .project_charge()
                .expect("the exact project index charge seals"),
            super::super::accounting::Charge {
                bytes: (MAX_PROJECT_BUCKETS as u64 + 2 * MAX_PROJECT_REFERENCES as u64)
                    * super::super::control::directory_charge().bytes,
                allocations: 0,
                members: MAX_PROJECT_BUCKETS as u64 + 2 * MAX_PROJECT_REFERENCES as u64,
            }
        );

        for (resource, inventory) in [
            (
                "typed store project candidate index buckets",
                Inventory {
                    project_buckets: MAX_PROJECT_BUCKETS + 1,
                    ..exact
                },
            ),
            (
                "typed store project candidate index references",
                Inventory {
                    project_references: MAX_PROJECT_REFERENCES + 1,
                    ..exact
                },
            ),
            (
                "typed store project candidate index bytes",
                Inventory {
                    project_bytes: MAX_PROJECT_BYTES + 1,
                    ..exact
                },
            ),
        ] {
            assert!(matches!(
                inventory.enforce(),
                Err(Error::Limit {
                    resource: observed,
                    actual,
                    ..
                }) if observed == resource && actual > 0
            ));
        }
    }
}

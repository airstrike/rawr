//! Historical namespace framing only; decoded selection is not current authority.

use std::path::{Component, Path, PathBuf};

use super::super::codec::{Reader, Writer};
use super::super::{Budget, Error, Result};
use crate::extract::plan::namespace::{Facts, Member, Retained};

const MAGIC: &[u8] = b"rawr-compiler-namespace\0\x01";
const MAX: usize = crate::store::bundle::MAX_MEMBERS as usize;

pub(super) fn write(writer: &mut Writer<'_>, retained: &Retained) -> Result<()> {
    validate_basic(retained)?;
    writer.raw(MAGIC)?;
    for paths in [&retained.roots, &retained.explicit] {
        writer.count(paths.len())?;
        for path in paths {
            writer.path(path)?;
        }
    }
    writer.count(retained.members.len())?;
    for member in &retained.members {
        writer.path(&member.path)?;
        writer.byte(member.facts.kind)?;
        writer.u32(member.facts.mode)?;
        for value in [
            member.facts.length,
            member.facts.allocated,
            member.facts.device,
            member.facts.inode,
            member.facts.links,
        ] {
            writer.raw(&value.to_le_bytes())?;
        }
        writer.byte(u8::from(member.candidate))?;
        writer.byte(u8::from(member.explicit))?;
        writer.byte(u8::from(member.selected))?;
        writer.byte(u8::from(member.digest.is_some()))?;
        if let Some(digest) = &member.digest {
            writer.raw(digest)?;
        }
    }
    Ok(())
}

pub(super) fn decode(bytes: &[u8], budget: &mut Budget<'_>) -> Result<Retained> {
    let result = (|| {
        budget.wire_section(bytes)?;
        let mut reader = Reader::new(bytes, budget);
        if reader.take(MAGIC.len())? != MAGIC {
            return Err(bad("unsupported namespace schema"));
        }
        let mut remaining = MAX;
        let roots = paths(&mut reader, &mut remaining)?;
        let explicit = paths(&mut reader, &mut remaining)?;
        let count = count(&mut reader, &mut remaining)?;
        if count > reader.remaining() / 57 {
            return Err(bad("impossible namespace member count"));
        }
        let mut members = reader.budget.items::<Member>(count)?;
        for _ in 0..count {
            let path = reader.path()?;
            let facts = Facts {
                kind: reader.byte()?,
                mode: reader.u32()?,
                length: u64_value(&mut reader)?,
                allocated: u64_value(&mut reader)?,
                device: u64_value(&mut reader)?,
                inode: u64_value(&mut reader)?,
                links: u64_value(&mut reader)?,
            };
            let candidate = flag(&mut reader)?;
            let explicit = flag(&mut reader)?;
            let selected = flag(&mut reader)?;
            let digest = if flag(&mut reader)? {
                Some(
                    reader
                        .take(32)?
                        .try_into()
                        .map_err(|_| bad("namespace digest truncated"))?,
                )
            } else {
                None
            };
            members.push(Member {
                path,
                facts,
                candidate,
                explicit,
                digest,
                selected,
            });
        }
        reader.finish()?;
        let retained = Retained {
            roots,
            explicit,
            members,
        };
        validate_basic(&retained)?;
        validate_roles(&retained, budget)?;
        Ok(retained)
    })();
    result.map_err(|error| budget.failure(error))
}

fn paths(reader: &mut Reader<'_, '_, '_>, remaining: &mut usize) -> Result<Vec<PathBuf>> {
    let count = count(reader, remaining)?;
    if count > reader.remaining() / 8 {
        return Err(bad("impossible namespace path count"));
    }
    let mut paths = reader.budget.items::<PathBuf>(count)?;
    for _ in 0..count {
        paths.push(reader.path()?);
    }
    Ok(paths)
}
fn count(reader: &mut Reader<'_, '_, '_>, remaining: &mut usize) -> Result<usize> {
    let count = reader.count()?;
    *remaining = remaining
        .checked_sub(count)
        .ok_or_else(|| bad("namespace aggregate member limit exceeded"))?;
    Ok(count)
}
fn flag(reader: &mut Reader<'_, '_, '_>) -> Result<bool> {
    match reader.byte()? {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(bad("invalid namespace boolean")),
    }
}
fn u64_value(reader: &mut Reader<'_, '_, '_>) -> Result<u64> {
    Ok(u64::from_le_bytes(
        reader
            .take(8)?
            .try_into()
            .map_err(|_| bad("namespace integer truncated"))?,
    ))
}
fn validate_basic(retained: &Retained) -> Result<()> {
    let count = retained
        .roots
        .len()
        .checked_add(retained.explicit.len())
        .and_then(|count| count.checked_add(retained.members.len()))
        .ok_or(Error::Overflow)?;
    if count > MAX {
        return Err(bad("namespace aggregate member limit exceeded"));
    }
    for path in retained.roots.iter().chain(&retained.explicit) {
        absolute(path)?;
    }
    let mut previous: Option<&Path> = None;
    for member in &retained.members {
        absolute(&member.path)?;
        if previous.is_some_and(|previous| previous.as_os_str() >= member.path.as_os_str()) {
            return Err(bad("namespace members are not strictly ordered"));
        }
        previous = Some(&member.path);
        let kind_mode = match member.facts.kind {
            0 => 0o040000,
            1 => 0o100000,
            _ => return Err(bad("invalid namespace member kind")),
        };
        if member.facts.mode & 0o170000 != kind_mode
            || member.facts.mode & !0o177777 != 0
            || member.facts.links == 0
            || member.facts.kind == 1 && member.facts.links != 1
            || member.facts.allocated % 512 != 0
        {
            return Err(bad("invalid namespace physical facts"));
        }
        let candidate = member.facts.kind == 1 && (member.explicit || metadata_name(&member.path));
        if member.candidate != candidate
            || member.selected != member.digest.is_some()
            || member.selected && !member.candidate
            || member.explicit && !member.selected
        {
            return Err(bad("invalid namespace content roles"));
        }
    }
    Ok(())
}
fn validate_roles(retained: &Retained, budget: &mut Budget<'_>) -> Result<()> {
    let mut roots = budget.items::<&Path>(retained.roots.len())?;
    roots.extend(retained.roots.iter().map(PathBuf::as_path));
    roots.sort_unstable_by(|a, b| a.as_os_str().cmp(b.as_os_str()));
    let mut explicit = budget.items::<&Path>(retained.explicit.len())?;
    explicit.extend(retained.explicit.iter().map(PathBuf::as_path));
    explicit.sort_unstable_by(|a, b| a.as_os_str().cmp(b.as_os_str()));
    let contains = |paths: &[&Path], path: &Path| {
        paths
            .binary_search_by(|candidate| candidate.as_os_str().cmp(path.as_os_str()))
            .is_ok()
    };
    for member in &retained.members {
        let is_root = contains(&roots, &member.path);
        let is_explicit = contains(&explicit, &member.path);
        if member.explicit != is_explicit
            || is_root && member.facts.kind != 0
            || is_explicit && member.facts.kind != 1
            || !(is_root
                || is_explicit
                || member
                    .path
                    .parent()
                    .is_some_and(|parent| contains(&roots, parent)))
        {
            return Err(bad(
                "namespace member has no matching root or explicit role",
            ));
        }
    }
    for path in roots.iter().chain(&explicit) {
        if retained
            .members
            .binary_search_by(|member| member.path.as_os_str().cmp(path.as_os_str()))
            .is_err()
        {
            return Err(bad("namespace declared path is absent"));
        }
    }
    Ok(())
}
fn absolute(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(bad("namespace path is relative or traverses a parent"));
    }
    Ok(())
}
fn metadata_name(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_none_or(|name| {
            [".rlib", ".rmeta", ".rs", ".dylib", ".so"]
                .iter()
                .any(|suffix| name.ends_with(suffix))
        })
}
fn bad(reason: &'static str) -> Error {
    Error::Encoding(reason)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::stored::codec;

    fn fixture() -> Retained {
        let facts = |kind, mode, length, links| Facts {
            kind,
            mode,
            length,
            allocated: 4096,
            device: 1,
            inode: u64::from(kind) + 1,
            links,
        };
        Retained {
            roots: vec![PathBuf::from("/root")],
            explicit: vec![PathBuf::from("/root/a.rmeta")],
            members: vec![
                Member {
                    path: PathBuf::from("/root"),
                    facts: facts(0, 0o040755, 64, 2),
                    candidate: false,
                    explicit: false,
                    selected: false,
                    digest: None,
                },
                Member {
                    path: PathBuf::from("/root/a.rmeta"),
                    facts: facts(1, 0o100644, 4, 1),
                    candidate: true,
                    explicit: true,
                    selected: true,
                    digest: Some([7; 32]),
                },
            ],
        }
    }
    fn with_budget<T>(operation: impl FnOnce(&mut Budget<'_>) -> T) -> T {
        let limits = crate::cargo::Limits::standard();
        let mut allocation = crate::input::Allocation::for_replay(&limits);
        let graph = crate::node::GraphBudget::new();
        let mut budget = Budget::borrow(&limits, &mut allocation, &graph);
        operation(&mut budget)
    }
    #[test]
    fn roundtrip_preserves_physical_values_and_native_paths() {
        use std::os::unix::ffi::OsStringExt as _;
        let mut retained = fixture();
        let path = PathBuf::from(std::ffi::OsString::from_vec(b"/root/a-\xff.rmeta".to_vec()));
        retained.explicit[0] = path.clone();
        retained.members[1].path = path;
        retained.members[1].facts.device = u64::MAX;
        let bytes =
            with_budget(|budget| codec::encode(budget, |writer| write(writer, &retained))).unwrap();
        let decoded = with_budget(|budget| decode(&bytes, budget)).unwrap();
        assert_eq!(decoded.roots, retained.roots);
        assert_eq!(decoded.explicit, retained.explicit);
        assert_eq!(decoded.members[1].facts, retained.members[1].facts);
        assert_eq!(decoded.members[1].digest, Some([7; 32]));
        let encoded =
            with_budget(|budget| codec::encode(budget, |writer| write(writer, &decoded))).unwrap();
        assert_eq!(encoded, bytes);
    }
    #[test]
    fn every_truncation_and_foreign_role_rejects() {
        let retained = fixture();
        let bytes =
            with_budget(|budget| codec::encode(budget, |writer| write(writer, &retained))).unwrap();
        for length in 0..bytes.len() {
            assert!(with_budget(|budget| decode(&bytes[..length], budget)).is_err());
        }
        let mut extra = bytes.clone();
        extra.push(0);
        assert!(with_budget(|budget| decode(&extra, budget)).is_err());
        let mut foreign = fixture();
        foreign.explicit[0] = PathBuf::from("/root/absent");
        let bytes =
            with_budget(|budget| codec::encode(budget, |writer| write(writer, &foreign))).unwrap();
        assert!(with_budget(|budget| decode(&bytes, budget)).is_err());
    }
    #[test]
    fn bad_order_modes_links_and_digest_combinations_reject() {
        let mut retained = fixture();
        retained.members.swap(0, 1);
        assert!(validate_basic(&retained).is_err());
        let mut retained = fixture();
        retained.members[1].facts.mode = 0o120777;
        assert!(validate_basic(&retained).is_err());
        let mut retained = fixture();
        retained.members[1].facts.links = 2;
        assert!(validate_basic(&retained).is_err());
        let mut retained = fixture();
        retained.members[1].digest = None;
        assert!(validate_basic(&retained).is_err());
    }
}

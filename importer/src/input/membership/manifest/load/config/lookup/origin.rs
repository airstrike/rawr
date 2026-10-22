//! The reached Value<String> definition wire round-trip, before path resolution.

use super::super::super::super::{session::Session, Diagnostic, Error, Step, Tag};
use super::super::definition::Definition;
use rawr_toml::document;
use std::{
    alloc::Layout,
    borrow::Cow,
    ffi::OsString,
    os::unix::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};

pub(super) enum Origin<'a> {
    Path(Cow<'a, Path>),
    Environment(&'a str),
    Cli(Option<Cow<'a, Path>>),
}

impl<'a> Origin<'a> {
    pub(super) fn decode<A: document::Admission>(
        definition: Definition<'a>,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.items(1)?;
        match definition {
            // Pinned ValueDeserializer emits integer 0, but Definition's
            // Deserialize expects (u32, String): this is an ordinary type error.
            Definition::BuiltIn => session.reject(Diagnostic {
                tag: Tag::ConfigDefinition,
                span: None,
                expected: Some("tuple"),
            }),
            Definition::Path(path) => Ok(Self::Path(lossy(path, session)?)),
            Definition::Environment(key) => {
                session.string(key.len())?;
                Ok(Self::Environment(key))
            }
            Definition::Cli(None) => Ok(Self::Cli(None)),
            Definition::Cli(Some(path)) => {
                let path = lossy(path, session)?;
                Ok(Self::Cli(if path.as_os_str().is_empty() {
                    None
                } else {
                    Some(path)
                }))
            }
        }
    }

    pub(super) fn definition(&self) -> Definition<'_> {
        match self {
            Self::Path(path) => Definition::Path(path),
            Self::Environment(key) => Definition::Environment(key),
            Self::Cli(path) => Definition::Cli(path.as_deref()),
        }
    }
}

fn lossy<'a, A: document::Admission>(
    path: &'a Path,
    session: &mut Session<'_, A>,
) -> Step<Cow<'a, Path>> {
    session.check()?;
    let bytes = path.as_os_str().as_bytes();
    if std::str::from_utf8(bytes).is_ok() {
        session.string(bytes.len())?;
        return Ok(Cow::Borrowed(path));
    }
    let mut length = 0usize;
    for chunk in bytes.utf8_chunks() {
        let Some(next) = length
            .checked_add(chunk.valid().len())
            .and_then(|n| n.checked_add(if chunk.invalid().is_empty() { 0 } else { 3 }))
        else {
            return session.fail(Error::Overflow);
        };
        length = next;
    }
    session.string(length)?;
    let Ok(layout) = Layout::array::<u8>(length) else {
        return session.fail(Error::Overflow);
    };
    session.admit(document::Request::Allocation {
        bytes: layout.size(),
        alignment: layout.align(),
    })?;
    let mut output = Vec::new();
    if let Err(error) = output.try_reserve_exact(length) {
        return session.fail(Error::Allocation(error));
    }
    for chunk in bytes.utf8_chunks() {
        output.extend_from_slice(chunk.valid().as_bytes());
        if !chunk.invalid().is_empty() {
            output.extend_from_slice("�".as_bytes());
        }
    }
    Ok(Cow::Owned(PathBuf::from(OsString::from_vec(output))))
}

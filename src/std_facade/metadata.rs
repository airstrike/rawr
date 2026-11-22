//! std's `fs::Metadata`, `fs::FileType` and `fs::DirEntry` for Rawr code:
//! what the file system says of a path, a file or a directory's entry,
//! each Rust's own value. `fs::metadata` and `fs::read_dir` give them, as
//! far as the calling program's access reads (`super::fs`).

use std::fs::{FileType, Metadata};
use std::sync::Arc;

use super::held::{self, Held};
use crate::{capability, Dynamic, EvalAltResult, Position};

/// The types the module holds.
pub(super) const HELD: [Held; 3] = [
    Held::Value(held::Value {
        std: "std::fs::Metadata",
        parameters: &[],
        lent: &[],
        also: &[],
        register: |builder, path, parameters, capabilities| {
            builder
                .type_::<Metadata>(path, parameters, capabilities)
                .map(drop)
        },
        crosses: false,
        runs: &[
            capability::Kind::Debug,
            capability::Kind::Clone,
            capability::Kind::Send,
            capability::Kind::Static,
        ],
    }),
    Held::Value(held::Value {
        std: "std::fs::FileType",
        parameters: &[],
        lent: &[],
        also: &[],
        register: |builder, path, parameters, capabilities| {
            builder
                .value_type::<FileType>(path, parameters, capabilities)
                .map(drop)
        },
        crosses: false,
        runs: &[
            capability::Kind::Debug,
            capability::Kind::Clone,
            capability::Kind::Copy,
            capability::Kind::PartialEq,
            capability::Kind::Eq,
            capability::Kind::Hash,
            capability::Kind::Send,
            capability::Kind::Static,
        ],
    }),
    Held::Value(held::Value {
        std: "std::fs::DirEntry",
        parameters: &[],
        lent: &[],
        also: &[],
        register: |builder, path, parameters, capabilities| {
            builder
                .type_::<DirEntry>(path, parameters, capabilities)
                .map(drop)
        },
        crosses: false,
        runs: &[
            capability::Kind::Debug,
            capability::Kind::Send,
            capability::Kind::Static,
        ],
    }),
];

/// std's `DirEntry`, which is not `Clone`: a Rawr value is a handle to it.
#[derive(Clone)]
pub(crate) struct DirEntry(Arc<std::fs::DirEntry>);

impl std::fmt::Debug for DirEntry {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Debug::fmt(&*self.0, formatter)
    }
}

/// Register `Debug` for each type.
pub(crate) fn register_debug() {
    crate::host::debug::register::<Metadata>();
    crate::host::debug::register::<FileType>();
    crate::host::debug::register::<DirEntry>();
}

/// A Rawr `Metadata` holding `metadata`.
pub(super) fn made(metadata: Metadata) -> Dynamic {
    Dynamic::from(metadata)
}

/// A directory's entries, each an `io::Result<DirEntry>`, as the iterator
/// `fs::read_dir` gives.
pub(super) fn entries(directory: std::fs::ReadDir) -> Dynamic {
    Dynamic::from(
        directory
            .map(|entry| match entry {
                Ok(entry) => crate::result::ok(Dynamic::from(DirEntry(Arc::new(entry)))),
                Err(failure) => crate::result::err(super::io::made(failure)),
            })
            .collect::<crate::Array>(),
    )
}

fn read<T: Clone + 'static>(value: &Dynamic, expected: &str) -> Result<T, Box<EvalAltResult>> {
    value
        .read_lock::<T>()
        .map(|value| value.clone())
        .ok_or_else(|| {
            EvalAltResult::ErrorRuntime(
                format!("expected `{expected}`, found `{}`", value.type_name()).into(),
                Position::NONE,
            )
            .into()
        })
}

/// An `io::Result` of `result`, its value made by `made`.
fn io_result<T>(result: std::io::Result<T>, made: impl FnOnce(T) -> Dynamic) -> Dynamic {
    match result {
        Ok(value) => crate::result::ok(made(value)),
        Err(failure) => crate::result::err(super::io::made(failure)),
    }
}

/// Bind the types' items.
pub(super) fn bind(binds: &mut impl super::Bind) {
    type Callback = fn(&mut [&mut Dynamic]) -> Result<Dynamic, Box<EvalAltResult>>;
    let mut bind = |path: &str, callback: Callback| binds.plain(path, callback);
    bind("fs::Metadata::len", |args| {
        let length = read::<Metadata>(args[0], "Metadata")?.len();
        Ok(Dynamic::from_int(
            crate::INT::try_from(length).unwrap_or(crate::INT::MAX),
        ))
    });
    bind("fs::Metadata::is_file", |args| {
        Ok(Dynamic::from_bool(
            read::<Metadata>(args[0], "Metadata")?.is_file(),
        ))
    });
    bind("fs::Metadata::is_dir", |args| {
        Ok(Dynamic::from_bool(
            read::<Metadata>(args[0], "Metadata")?.is_dir(),
        ))
    });
    bind("fs::Metadata::is_symlink", |args| {
        Ok(Dynamic::from_bool(
            read::<Metadata>(args[0], "Metadata")?.is_symlink(),
        ))
    });
    bind("fs::Metadata::file_type", |args| {
        Ok(Dynamic::from(
            read::<Metadata>(args[0], "Metadata")?.file_type(),
        ))
    });
    bind("fs::Metadata::modified", |args| {
        Ok(io_result(
            read::<Metadata>(args[0], "Metadata")?.modified(),
            Dynamic::from,
        ))
    });
    bind("fs::Metadata::accessed", |args| {
        Ok(io_result(
            read::<Metadata>(args[0], "Metadata")?.accessed(),
            Dynamic::from,
        ))
    });
    bind("fs::Metadata::created", |args| {
        Ok(io_result(
            read::<Metadata>(args[0], "Metadata")?.created(),
            Dynamic::from,
        ))
    });
    bind("fs::FileType::is_file", |args| {
        Ok(Dynamic::from_bool(
            read::<FileType>(args[0], "FileType")?.is_file(),
        ))
    });
    bind("fs::FileType::is_dir", |args| {
        Ok(Dynamic::from_bool(
            read::<FileType>(args[0], "FileType")?.is_dir(),
        ))
    });
    bind("fs::FileType::is_symlink", |args| {
        Ok(Dynamic::from_bool(
            read::<FileType>(args[0], "FileType")?.is_symlink(),
        ))
    });
    bind("fs::DirEntry::path", |args| {
        Ok(Dynamic::from(
            read::<DirEntry>(args[0], "DirEntry")?.0.path(),
        ))
    });
    bind("fs::DirEntry::file_name", |args| {
        Ok(Dynamic::from(
            read::<DirEntry>(args[0], "DirEntry")?.0.file_name(),
        ))
    });
    bind("fs::DirEntry::metadata", |args| {
        Ok(io_result(
            read::<DirEntry>(args[0], "DirEntry")?.0.metadata(),
            made,
        ))
    });
    bind("fs::DirEntry::file_type", |args| {
        Ok(io_result(
            read::<DirEntry>(args[0], "DirEntry")?.0.file_type(),
            Dynamic::from,
        ))
    });
}

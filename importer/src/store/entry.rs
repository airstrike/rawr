#[cfg(feature = "import-baseline")]
use crate::metrics::principal::temporary;

use std::fs::{self, File, OpenOptions};
use std::io::{Read as _, Write as _};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::node::Error;

#[cfg(test)]
mod tests;

static RESERVATION: AtomicU64 = AtomicU64::new(1);

pub(crate) enum Publication {
    Published,
    Existing,
}

pub(crate) fn ensure_directory(
    root: &Path,
    relative: &Path,
    #[cfg(feature = "import-baseline")] graph: &crate::node::GraphBudget,
) -> Result<PathBuf, Error> {
    ensure_directory_with(
        None,
        root,
        relative,
        #[cfg(feature = "import-baseline")]
        crate::metrics::principal::creation::Context::unleased(
            root,
            graph,
            crate::metrics::principal::creation::Source::Directory,
        ),
    )
}

/// `<target>/rawr/store`, the project's typed store.
pub(crate) fn ensure_project_root(
    rawr: &Path,
    #[cfg(feature = "import-baseline")] graph: &crate::node::GraphBudget,
) -> Result<PathBuf, Error> {
    let root = fs::create_dir_all(rawr)
        .and_then(|()| fs::canonicalize(rawr))
        .map_err(|source| Error::Io {
            operation: "create the project's Rawr directory",
            path: rawr.to_path_buf(),
            source,
        })?;
    ensure_directory_with(
        None,
        &root,
        Path::new("store"),
        #[cfg(feature = "import-baseline")]
        crate::metrics::principal::creation::Context::unleased(
            &root,
            graph,
            crate::metrics::principal::creation::Source::CargoHome,
        ),
    )
}

pub(crate) fn ensure_admitted_directory(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    relative: &Path,
) -> Result<PathBuf, Error> {
    ensure_directory_with(
        Some(publication),
        root,
        relative,
        #[cfg(feature = "import-baseline")]
        crate::metrics::principal::creation::Context::published(
            publication,
            crate::metrics::principal::creation::Source::Directory,
        ),
    )
}

fn ensure_directory_with(
    publication: Option<&super::lease::Publication<'_>>,
    root: &Path,
    relative: &Path,
    #[cfg(feature = "import-baseline")] observer: crate::metrics::principal::creation::Context<'_>,
) -> Result<PathBuf, Error> {
    #[cfg(feature = "import-baseline")]
    use crate::metrics::principal::creation;
    if relative.is_absolute() {
        return Err(Error::OutsideRoot(relative.to_path_buf()));
    }
    let mut current = root.to_path_buf();
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(Error::OutsideRoot(relative.to_path_buf()));
        };
        current.push(component);
        #[cfg(feature = "import-baseline")]
        let observation = observer.start(&current, true);
        match fs::symlink_metadata(&current) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() || !metadata.is_dir() {
                    return Err(Error::Shape(current));
                }
                #[cfg(feature = "import-baseline")]
                observation.adopted(&metadata);
            }
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => {
                if let Some(publication) = publication {
                    let admitted = super::accounting::admit_control(
                        publication,
                        root,
                        super::control::Plan::directory(&current),
                    )?;
                    admitted.consume(publication, root, "directory", &current, 0)?;
                }
                match fs::create_dir(&current) {
                    Ok(()) => {
                        #[cfg(feature = "import-baseline")]
                        observation.created(creation::Outcome::Created);
                    }
                    Err(source) if source.kind() == std::io::ErrorKind::AlreadyExists => {
                        #[cfg(feature = "import-baseline")]
                        observation.created(creation::Outcome::Contended);
                        let metadata =
                            fs::symlink_metadata(&current).map_err(|source| Error::Io {
                                operation: "inspect concurrently created typed store directory",
                                path: current.clone(),
                                source,
                            })?;
                        if metadata.file_type().is_symlink() || !metadata.is_dir() {
                            return Err(Error::Shape(current));
                        }
                        #[cfg(feature = "import-baseline")]
                        observation.adopted(&metadata);
                    }
                    Err(source) => {
                        #[cfg(feature = "import-baseline")]
                        observation.created(creation::Outcome::Failed);
                        return Err(Error::Io {
                            operation: "create typed store directory",
                            path: current,
                            source,
                        });
                    }
                }
            }
            Err(source) => {
                return Err(Error::Io {
                    operation: "inspect typed store directory",
                    path: current,
                    source,
                });
            }
        }
        #[cfg(feature = "import-baseline")]
        observation.completed(true);
    }
    let canonical = fs::canonicalize(&current).map_err(|source| Error::Io {
        operation: "canonicalize typed store directory",
        path: current,
        source,
    })?;
    if !canonical.starts_with(root) {
        return Err(Error::OutsideRoot(canonical));
    }
    Ok(canonical)
}

pub(crate) fn existing_directory(root: &Path, relative: &Path) -> Result<Option<PathBuf>, Error> {
    existing_directory_with_budget(root, relative, None)
}

pub(crate) fn existing_directory_budgeted(
    root: &Path,
    relative: &Path,
    budget: &crate::node::GraphBudget,
) -> Result<Option<PathBuf>, Error> {
    existing_directory_with_budget(root, relative, Some(budget))
}

fn existing_directory_with_budget(
    root: &Path,
    relative: &Path,
    budget: Option<&crate::node::GraphBudget>,
) -> Result<Option<PathBuf>, Error> {
    if relative.is_absolute() {
        return Err(Error::OutsideRoot(relative.to_path_buf()));
    }
    let length = root
        .as_os_str()
        .as_encoded_bytes()
        .len()
        .checked_add(relative.as_os_str().as_encoded_bytes().len())
        .and_then(|size| size.checked_add(1))
        .ok_or(Error::Allocation)?;
    if let Some(budget) = budget {
        budget.decode(std::mem::size_of::<PathBuf>() + length)?;
    }
    let mut current = PathBuf::new();
    current
        .try_reserve_exact(length)
        .map_err(|_| Error::Allocation)?;
    current.push(root);
    for component in relative.components() {
        let Component::Normal(component) = component else {
            return Err(Error::OutsideRoot(relative.to_path_buf()));
        };
        current.push(component);
        if let Some(budget) = budget {
            budget.paths(1)?;
            super::path::input(&current, budget)?;
        }
        let metadata = match fs::symlink_metadata(&current) {
            Ok(metadata) => metadata,
            Err(source) if source.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(source) => {
                return Err(Error::Io {
                    operation: "inspect typed store directory",
                    path: current,
                    source,
                });
            }
        };
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(Error::Shape(current));
        }
    }
    if let Some(budget) = budget {
        budget.paths(1)?;
        super::path::canonical(&current, budget)?;
    }
    let canonical = fs::canonicalize(&current).map_err(|source| Error::Io {
        operation: "canonicalize typed store directory",
        path: current,
        source,
    })?;
    if !canonical.starts_with(root) {
        return Err(Error::OutsideRoot(canonical));
    }
    Ok(Some(canonical))
}

pub(crate) fn read_file(path: &Path, limit: usize) -> Result<Box<[u8]>, Error> {
    read_file_with_budget(path, limit, None, None, |actual| file_limit(limit, actual))
}

pub(crate) fn read_file_budgeted(
    path: &Path,
    limit: usize,
    budget: &crate::node::GraphBudget,
) -> Result<Box<[u8]>, Error> {
    read_file_budgeted_with(path, limit, budget, |actual| file_limit(limit, actual))
}

/// Only this reader's observed file extent may use the supplied rejection.
/// Account, allocation and I/O failures retain their original typed errors.
pub(super) fn read_file_budgeted_with(
    path: &Path,
    limit: usize,
    budget: &crate::node::GraphBudget,
    oversized: impl FnOnce(usize) -> Error,
) -> Result<Box<[u8]>, Error> {
    read_file_with_budget(path, limit, Some(budget), None, oversized)
}

pub(super) fn read_candidate_file_budgeted(
    path: &Path,
    limit: usize,
    budget: &crate::node::GraphBudget,
) -> Result<Box<[u8]>, Error> {
    read_file_budgeted_with(path, limit, budget, |_| Error::Corrupt {
        path: path.to_path_buf(),
        reason: "candidate stored file exceeds its extent bound",
    })
}

/// Read exact typed output within its transform bound, outside the metadata ceiling.
/// Its allocation/content hashes must be charged before the typed decoder runs.
pub(crate) fn read_payload_budgeted(
    path: &Path,
    content_length: usize,
    output_limit: usize,
    budget: &crate::node::GraphBudget,
) -> Result<Box<[u8]>, Error> {
    read_file_with_budget(
        path,
        output_limit,
        Some(budget),
        Some(content_length),
        |actual| file_limit(output_limit, actual),
    )
}

/// The shape and extent checks `read_payload_budgeted` applies before reading,
/// without reading: the regular, non-symlink, singly linked file whose length
/// equals its envelope's content length and fits its output limit.
pub(crate) fn inspect_payload_budgeted(
    path: &Path,
    content_length: usize,
    output_limit: usize,
    budget: &crate::node::GraphBudget,
) -> Result<fs::Metadata, Error> {
    budget.paths(1)?;
    super::path::input(path, budget)?;
    let metadata = fs::symlink_metadata(path).map_err(|source| Error::Io {
        operation: "inspect typed store file",
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || hard_linked(&metadata) {
        return Err(Error::Shape(path.to_path_buf()));
    }
    let length = usize::try_from(metadata.len()).unwrap_or(usize::MAX);
    if length > output_limit {
        return Err(file_limit(output_limit, length));
    }
    if content_length != length {
        return Err(Error::Corrupt {
            path: path.to_path_buf(),
            reason: "typed payload length differs from its envelope",
        });
    }
    Ok(metadata)
}

pub(super) fn read_candidate_payload_budgeted(
    path: &Path,
    content_length: usize,
    output_limit: usize,
    budget: &crate::node::GraphBudget,
) -> Result<Box<[u8]>, Error> {
    read_file_with_budget(
        path,
        output_limit,
        Some(budget),
        Some(content_length),
        |_| Error::Corrupt {
            path: path.to_path_buf(),
            reason: "candidate typed payload exceeds its extent bound",
        },
    )
}

fn file_limit(limit: usize, actual: usize) -> Error {
    Error::Limit {
        resource: "typed store file bytes",
        limit,
        actual,
    }
}

fn read_file_with_budget(
    path: &Path,
    limit: usize,
    budget: Option<&crate::node::GraphBudget>,
    content_length: Option<usize>,
    oversized: impl FnOnce(usize) -> Error,
) -> Result<Box<[u8]>, Error> {
    if let Some(budget) = budget {
        budget.paths(1)?;
        super::path::input(path, budget)?;
    }
    let metadata = fs::symlink_metadata(path).map_err(|source| Error::Io {
        operation: "inspect typed store file",
        path: path.to_path_buf(),
        source,
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_file() || hard_linked(&metadata) {
        return Err(Error::Shape(path.to_path_buf()));
    }
    let length = match usize::try_from(metadata.len()) {
        Ok(length) => length,
        Err(_) => return Err(oversized(usize::MAX)),
    };
    if length > limit {
        return Err(oversized(length));
    }
    if content_length.is_some_and(|expected| expected != length) {
        return Err(Error::Corrupt {
            path: path.to_path_buf(),
            reason: "typed payload length differs from its envelope",
        });
    }
    if let Some(budget) = budget {
        if content_length.is_none() {
            budget.decode(length)?;
        }
        budget.paths(1)?;
        super::path::input(path, budget)?;
    }
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| Error::Allocation)?;
    bytes.resize(length, 0);
    let mut source = File::open(path).map_err(|source| Error::Io {
        operation: "open typed store file",
        path: path.to_path_buf(),
        source,
    })?;
    source.read_exact(&mut bytes).map_err(|source| {
        if source.kind() == std::io::ErrorKind::UnexpectedEof {
            Error::Corrupt {
                path: path.to_path_buf(),
                reason: "file length changed while it was read",
            }
        } else {
            Error::Io {
                operation: "read typed store file",
                path: path.to_path_buf(),
                source,
            }
        }
    })?;
    let mut extra = [0_u8; 1];
    if source.read(&mut extra).map_err(|source| Error::Io {
        operation: "inspect typed store file end",
        path: path.to_path_buf(),
        source,
    })? != 0
    {
        return Err(Error::Corrupt {
            path: path.to_path_buf(),
            reason: "file length changed while it was read",
        });
    }
    Ok(bytes.into_boxed_slice())
}

pub(crate) fn publish_directory(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    target: &Path,
    file_name: &'static str,
    bytes: &[u8],
    #[cfg(feature = "import-baseline")] class: crate::metrics::publication::Class,
    #[cfg(feature = "import-baseline")] principal: Option<&crate::metrics::principal::Attempt<'_>>,
    #[cfg(feature = "import-baseline")] pin: Option<&crate::metrics::principal::pin::Attempt>,
) -> Result<Publication, Error> {
    #[cfg(feature = "import-baseline")]
    let mut observation = crate::metrics::publication::Attempt::start(class);
    #[cfg(feature = "import-baseline")]
    let lifecycle = temporary::Attempt::start(
        principal
            .map(|principal| principal.temporary())
            .or_else(|| pin.and_then(|pin| pin.temporary())),
        temporary::Form::Directory,
        Some(file_name),
    );
    if !target.starts_with(root) {
        return Err(Error::OutsideRoot(target.to_path_buf()));
    }
    match fs::symlink_metadata(target) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            return Err(Error::Shape(target.to_path_buf()));
        }
        Ok(_) => {
            #[cfg(feature = "import-baseline")]
            observation.operation(Some(true));
            #[cfg(feature = "import-baseline")]
            if let Some(principal) = principal {
                principal.completed(true);
            }
            #[cfg(feature = "import-baseline")]
            {
                lifecycle.operation(temporary::Outcome::Existing);
                lifecycle.completed(temporary::Outcome::Existing);
            }
            return Ok(Publication::Existing);
        }
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(Error::Io {
                operation: "inspect typed store control publication",
                path: target.to_path_buf(),
                source,
            });
        }
    }
    let admitted = super::accounting::admit_control(
        publication,
        root,
        super::control::Plan::publication(target, bytes.len()),
    )?;
    admitted.consume(publication, root, "publication", target, bytes.len())?;
    let temporary_root = root.join("temporaries");
    let process = std::process::id();
    let sequence = RESERVATION.fetch_add(1, Ordering::Relaxed);
    let reservation = format!("{process}-{sequence}");
    #[cfg(feature = "import-baseline")]
    lifecycle.reserved(process, sequence);
    let temporary = temporary_root.join(reservation);
    #[cfg(feature = "import-baseline")]
    lifecycle.step(temporary::Action::CreateDirectory);
    let created = fs::create_dir(&temporary);
    #[cfg(feature = "import-baseline")]
    lifecycle.result(created.is_ok());
    created.map_err(|source| Error::Io {
        operation: "create typed store publication reservation",
        path: temporary.clone(),
        source,
    })?;
    #[cfg(feature = "import-baseline")]
    observation.created();
    let temporary_file = temporary.join(file_name);
    let result = (|| {
        #[cfg(feature = "import-baseline")]
        lifecycle.step(temporary::Action::Open(temporary::Member::Primary));
        let opened = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_file);
        #[cfg(feature = "import-baseline")]
        lifecycle.result(opened.is_ok());
        let mut output = opened.map_err(|source| Error::Io {
            operation: "create typed store temporary payload",
            path: temporary_file.clone(),
            source,
        })?;
        #[cfg(feature = "import-baseline")]
        lifecycle.step(temporary::Action::Write(temporary::Member::Primary));
        let written = output.write_all(bytes);
        #[cfg(feature = "import-baseline")]
        lifecycle.result(written.is_ok());
        written.map_err(|source| Error::Io {
            operation: "write typed store temporary payload",
            path: temporary_file.clone(),
            source,
        })?;
        #[cfg(feature = "import-baseline")]
        lifecycle.step(temporary::Action::SyncFile(temporary::Member::Primary));
        let synced = output.sync_all();
        #[cfg(feature = "import-baseline")]
        lifecycle.result(synced.is_ok());
        synced.map_err(|source| Error::Io {
            operation: "sync typed store temporary payload",
            path: temporary_file.clone(),
            source,
        })?;
        #[cfg(feature = "import-baseline")]
        lifecycle.step(temporary::Action::SyncTemporary);
        let synced = sync_directory(&temporary);
        #[cfg(feature = "import-baseline")]
        lifecycle.result(synced.is_ok());
        synced?;
        if target.exists() {
            return Ok(Publication::Existing);
        }
        #[cfg(feature = "import-baseline")]
        lifecycle.step(temporary::Action::Rename);
        let renamed = fs::rename(&temporary, target);
        #[cfg(feature = "import-baseline")]
        lifecycle.result(renamed.is_ok());
        match renamed {
            Ok(()) => {
                #[cfg(feature = "import-baseline")]
                observation.renamed();
                #[cfg(feature = "import-baseline")]
                if let Some(pin) = pin {
                    pin.step(crate::metrics::principal::pin::Step::Renamed);
                }
                #[cfg(feature = "import-baseline")]
                if let Some(principal) = principal {
                    principal.renamed();
                }
                if let Some(parent) = target.parent() {
                    #[cfg(feature = "import-baseline")]
                    lifecycle.step(temporary::Action::SyncParent);
                    let synced = sync_directory(parent);
                    #[cfg(feature = "import-baseline")]
                    lifecycle.result(synced.is_ok());
                    synced?;
                }
                Ok(Publication::Published)
            }
            Err(source)
                if matches!(
                    source.kind(),
                    std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::DirectoryNotEmpty
                ) =>
            {
                Ok(Publication::Existing)
            }
            Err(source) => Err(Error::Io {
                operation: "publish typed store entry",
                path: target.to_path_buf(),
                source,
            }),
        }
    })();
    #[cfg(feature = "import-baseline")]
    observation.operation(
        result
            .as_ref()
            .ok()
            .map(|value| matches!(value, Publication::Existing)),
    );
    #[cfg(feature = "import-baseline")]
    lifecycle.operation(match &result {
        Ok(Publication::Published) => temporary::Outcome::Published,
        Ok(Publication::Existing) => temporary::Outcome::Existing,
        Err(_) => temporary::Outcome::Failed,
    });
    let remains = temporary.exists();
    #[cfg(feature = "import-baseline")]
    lifecycle.cleanup_checked(remains);
    if remains {
        #[cfg(feature = "import-baseline")]
        observation.cleanup_started();
        #[cfg(feature = "import-baseline")]
        lifecycle.step(temporary::Action::Cleanup);
        let removed = fs::remove_dir_all(&temporary);
        #[cfg(feature = "import-baseline")]
        lifecycle.result(removed.is_ok());
        let cleanup = removed.map_err(|source| Error::Io {
            operation: "retire typed store publication reservation",
            path: temporary,
            source,
        });
        #[cfg(feature = "import-baseline")]
        observation.cleanup(cleanup.is_ok());
        cleanup?;
    }
    #[cfg(feature = "import-baseline")]
    if let (Some(principal), Ok(value)) = (principal, &result) {
        principal.completed(matches!(value, Publication::Existing));
    }
    #[cfg(feature = "import-baseline")]
    lifecycle.completed(match &result {
        Ok(Publication::Published) => temporary::Outcome::Published,
        Ok(Publication::Existing) => temporary::Outcome::Existing,
        Err(_) => temporary::Outcome::Failed,
    });
    result
}

pub(crate) fn publish_sealed_directory<T>(
    root: &Path,
    target: &Path,
    file_name: &'static str,
    bytes: &[u8],
    #[cfg(feature = "import-baseline")] class: crate::metrics::publication::Class,
    #[cfg(feature = "import-baseline")] principal: Option<&crate::metrics::principal::Attempt<'_>>,
    seal: impl FnOnce(&fs::Metadata) -> Result<(Box<[u8]>, T), Error>,
) -> Result<(Publication, Option<T>), Error> {
    publish_sealed_with(
        root,
        target,
        file_name,
        bytes,
        #[cfg(feature = "import-baseline")]
        class,
        #[cfg(feature = "import-baseline")]
        principal,
        seal,
        sync_directory,
    )
}

fn publish_sealed_with<T>(
    root: &Path,
    target: &Path,
    file_name: &'static str,
    bytes: &[u8],
    #[cfg(feature = "import-baseline")] class: crate::metrics::publication::Class,
    #[cfg(feature = "import-baseline")] principal: Option<&crate::metrics::principal::Attempt<'_>>,
    seal: impl FnOnce(&fs::Metadata) -> Result<(Box<[u8]>, T), Error>,
    synchronize: impl Fn(&Path) -> Result<(), Error>,
) -> Result<(Publication, Option<T>), Error> {
    #[cfg(feature = "import-baseline")]
    let mut observation = crate::metrics::publication::Attempt::start(class);
    #[cfg(feature = "import-baseline")]
    let lifecycle = temporary::Attempt::start(
        principal.map(|principal| principal.temporary()),
        temporary::Form::Sealed,
        Some(file_name),
    );
    if !target.starts_with(root) || file_name == "allocation" {
        return Err(Error::OutsideRoot(target.to_path_buf()));
    }
    let temporary_root = root.join("temporaries");
    let process = std::process::id();
    let sequence = RESERVATION.fetch_add(1, Ordering::Relaxed);
    let reservation = format!("{process}-{sequence}");
    #[cfg(feature = "import-baseline")]
    lifecycle.reserved(process, sequence);
    let temporary = temporary_root.join(reservation);
    #[cfg(feature = "import-baseline")]
    lifecycle.step(temporary::Action::CreateDirectory);
    let created = fs::create_dir(&temporary);
    #[cfg(feature = "import-baseline")]
    lifecycle.result(created.is_ok());
    created.map_err(|source| Error::Io {
        operation: "create sealed store publication reservation",
        path: temporary.clone(),
        source,
    })?;
    #[cfg(feature = "import-baseline")]
    observation.created();
    let temporary_file = temporary.join(file_name);
    let result = (|| {
        #[cfg(feature = "import-baseline")]
        lifecycle.step(temporary::Action::Open(temporary::Member::Primary));
        let opened = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary_file);
        #[cfg(feature = "import-baseline")]
        lifecycle.result(opened.is_ok());
        let mut output = opened.map_err(|source| Error::Io {
            operation: "create sealed store temporary payload",
            path: temporary_file.clone(),
            source,
        })?;
        #[cfg(feature = "import-baseline")]
        lifecycle.step(temporary::Action::Write(temporary::Member::Primary));
        let written = output.write_all(bytes);
        #[cfg(feature = "import-baseline")]
        lifecycle.result(written.is_ok());
        written.map_err(|source| Error::Io {
            operation: "write sealed store temporary payload",
            path: temporary_file.clone(),
            source,
        })?;
        #[cfg(feature = "import-baseline")]
        lifecycle.step(temporary::Action::SyncFile(temporary::Member::Primary));
        let synced = output.sync_all();
        #[cfg(feature = "import-baseline")]
        lifecycle.result(synced.is_ok());
        synced.map_err(|source| Error::Io {
            operation: "sync sealed store temporary payload",
            path: temporary_file.clone(),
            source,
        })?;
        let metadata = output.metadata().map_err(|source| Error::Io {
            operation: "inspect sealed store temporary payload",
            path: temporary_file.clone(),
            source,
        })?;
        let (record, retained) = seal(&metadata)?;
        let record_path = temporary.join("allocation");
        #[cfg(feature = "import-baseline")]
        lifecycle.step(temporary::Action::Open(temporary::Member::Allocation));
        let opened = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&record_path);
        #[cfg(feature = "import-baseline")]
        lifecycle.result(opened.is_ok());
        let mut record_file = opened.map_err(|source| Error::Io {
            operation: "create sealed store allocation record",
            path: record_path.clone(),
            source,
        })?;
        #[cfg(feature = "import-baseline")]
        lifecycle.step(temporary::Action::Write(temporary::Member::Allocation));
        let written = record_file.write_all(&record);
        #[cfg(feature = "import-baseline")]
        lifecycle.result(written.is_ok());
        written.map_err(|source| Error::Io {
            operation: "write sealed store allocation record",
            path: record_path.clone(),
            source,
        })?;
        #[cfg(feature = "import-baseline")]
        lifecycle.step(temporary::Action::SyncFile(temporary::Member::Allocation));
        let synced = record_file.sync_all();
        #[cfg(feature = "import-baseline")]
        lifecycle.result(synced.is_ok());
        synced.map_err(|source| Error::Io {
            operation: "sync sealed store allocation record",
            path: record_path,
            source,
        })?;
        #[cfg(feature = "import-baseline")]
        lifecycle.step(temporary::Action::SyncTemporary);
        let synced = synchronize(&temporary);
        #[cfg(feature = "import-baseline")]
        lifecycle.result(synced.is_ok());
        synced?;
        if target.exists() {
            return Ok((Publication::Existing, None));
        }
        #[cfg(feature = "import-baseline")]
        lifecycle.step(temporary::Action::Rename);
        let renamed = fs::rename(&temporary, target);
        #[cfg(feature = "import-baseline")]
        lifecycle.result(renamed.is_ok());
        match renamed {
            Ok(()) => {
                #[cfg(feature = "import-baseline")]
                observation.renamed();
                #[cfg(feature = "import-baseline")]
                if let Some(principal) = principal {
                    principal.renamed();
                }
                if let Some(parent) = target.parent() {
                    #[cfg(feature = "import-baseline")]
                    lifecycle.step(temporary::Action::SyncParent);
                    let synced = synchronize(parent);
                    #[cfg(feature = "import-baseline")]
                    lifecycle.result(synced.is_ok());
                    synced?;
                }
                Ok((Publication::Published, Some(retained)))
            }
            Err(source)
                if matches!(
                    source.kind(),
                    std::io::ErrorKind::AlreadyExists | std::io::ErrorKind::DirectoryNotEmpty
                ) =>
            {
                Ok((Publication::Existing, None))
            }
            Err(source) => Err(Error::Io {
                operation: "publish sealed store entry",
                path: target.to_path_buf(),
                source,
            }),
        }
    })();
    #[cfg(feature = "import-baseline")]
    observation.operation(
        result
            .as_ref()
            .ok()
            .map(|(value, _)| matches!(value, Publication::Existing)),
    );
    #[cfg(feature = "import-baseline")]
    lifecycle.operation(match &result {
        Ok((Publication::Published, _)) => temporary::Outcome::Published,
        Ok((Publication::Existing, _)) => temporary::Outcome::Existing,
        Err(_) => temporary::Outcome::Failed,
    });
    let remains = temporary.exists();
    #[cfg(feature = "import-baseline")]
    lifecycle.cleanup_checked(remains);
    if remains {
        #[cfg(feature = "import-baseline")]
        observation.cleanup_started();
        #[cfg(feature = "import-baseline")]
        lifecycle.step(temporary::Action::Cleanup);
        let removed = fs::remove_dir_all(&temporary);
        #[cfg(feature = "import-baseline")]
        lifecycle.result(removed.is_ok());
        let cleanup = removed.map_err(|source| Error::Io {
            operation: "retire sealed store publication reservation",
            path: temporary,
            source,
        });
        #[cfg(feature = "import-baseline")]
        observation.cleanup(cleanup.is_ok());
        cleanup?;
    }
    #[cfg(feature = "import-baseline")]
    if let (Some(principal), Ok((value, _))) = (principal, &result) {
        principal.completed(matches!(value, Publication::Existing));
    }
    #[cfg(feature = "import-baseline")]
    lifecycle.completed(match &result {
        Ok((Publication::Published, _)) => temporary::Outcome::Published,
        Ok((Publication::Existing, _)) => temporary::Outcome::Existing,
        Err(_) => temporary::Outcome::Failed,
    });
    result
}

pub(crate) fn replace_admitted_file(
    admitted: super::control::Admitted<'_>,
    publication: &super::lease::Publication<'_>,
    root: &Path,
    target: &Path,
    bytes: &[u8],
    #[cfg(feature = "import-baseline")] purpose: crate::metrics::publication::Replacement,
    #[cfg(feature = "import-baseline")] ledger: Option<
        crate::metrics::principal::ledger::Write<'_>,
    >,
) -> Result<(), Error> {
    replace_admitted_file_with(
        admitted,
        publication,
        root,
        target,
        bytes,
        #[cfg(feature = "import-baseline")]
        purpose,
        #[cfg(feature = "import-baseline")]
        ledger,
        sync_directory,
    )
}

fn replace_admitted_file_with(
    admitted: super::control::Admitted<'_>,
    publication: &super::lease::Publication<'_>,
    root: &Path,
    target: &Path,
    bytes: &[u8],
    #[cfg(feature = "import-baseline")] purpose: crate::metrics::publication::Replacement,
    #[cfg(feature = "import-baseline")] ledger: Option<
        crate::metrics::principal::ledger::Write<'_>,
    >,
    mut sync: impl FnMut(&Path) -> Result<(), Error>,
) -> Result<(), Error> {
    #[cfg(feature = "import-baseline")]
    let mut observation = crate::metrics::publication::Attempt::start(
        crate::metrics::publication::Class::Replacement(purpose),
    );
    #[cfg(feature = "import-baseline")]
    let ledger = ledger.map(|write| {
        crate::metrics::principal::ledger::Attempt::replace(
            publication,
            root,
            target,
            bytes,
            purpose,
            write,
        )
    });
    #[cfg(feature = "import-baseline")]
    let lifecycle = temporary::Attempt::start(
        ledger.as_ref().map(|ledger| ledger.temporary()),
        temporary::Form::Replacement,
        None,
    );
    admitted.consume(publication, root, "replacement", target, bytes.len())?;
    if !target.starts_with(root) {
        return Err(Error::OutsideRoot(target.to_path_buf()));
    }
    match fs::symlink_metadata(target) {
        Ok(metadata)
            if metadata.file_type().is_symlink()
                || !metadata.is_file()
                || hard_linked(&metadata) =>
        {
            return Err(Error::Shape(target.to_path_buf()));
        }
        Ok(_) => {}
        Err(source) if source.kind() == std::io::ErrorKind::NotFound => {}
        Err(source) => {
            return Err(Error::Io {
                operation: "inspect replaceable typed store file",
                path: target.to_path_buf(),
                source,
            });
        }
    }
    let process = std::process::id();
    let sequence = RESERVATION.fetch_add(1, Ordering::Relaxed);
    let reservation = format!("{process}-{sequence}");
    #[cfg(feature = "import-baseline")]
    lifecycle.reserved(process, sequence);
    let temporary = root.join("temporaries").join(reservation);
    let result = (|| {
        #[cfg(feature = "import-baseline")]
        lifecycle.step(temporary::Action::Open(temporary::Member::Primary));
        let opened = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary);
        #[cfg(feature = "import-baseline")]
        lifecycle.result(opened.is_ok());
        let mut output = opened.map_err(|source| Error::Io {
            operation: "create typed store replacement file",
            path: temporary.clone(),
            source,
        })?;
        #[cfg(feature = "import-baseline")]
        observation.created();
        #[cfg(feature = "import-baseline")]
        lifecycle.step(temporary::Action::Write(temporary::Member::Primary));
        let written = output.write_all(bytes);
        #[cfg(feature = "import-baseline")]
        lifecycle.result(written.is_ok());
        written.map_err(|source| Error::Io {
            operation: "write typed store replacement file",
            path: temporary.clone(),
            source,
        })?;
        #[cfg(feature = "import-baseline")]
        lifecycle.step(temporary::Action::SyncFile(temporary::Member::Primary));
        let synced = output.sync_all();
        #[cfg(feature = "import-baseline")]
        lifecycle.result(synced.is_ok());
        synced.map_err(|source| Error::Io {
            operation: "sync typed store replacement file",
            path: temporary.clone(),
            source,
        })?;
        #[cfg(feature = "import-baseline")]
        lifecycle.step(temporary::Action::Rename);
        let renamed = fs::rename(&temporary, target);
        #[cfg(feature = "import-baseline")]
        lifecycle.result(renamed.is_ok());
        renamed.map_err(|source| Error::Io {
            operation: "replace typed store ledger",
            path: target.to_path_buf(),
            source,
        })?;
        #[cfg(feature = "import-baseline")]
        observation.renamed();
        #[cfg(feature = "import-baseline")]
        if let Some(ledger) = &ledger {
            ledger.renamed();
        }
        if let Some(parent) = target.parent() {
            #[cfg(feature = "import-baseline")]
            lifecycle.step(temporary::Action::SyncParent);
            let synced = sync(parent);
            #[cfg(feature = "import-baseline")]
            lifecycle.result(synced.is_ok());
            synced?;
        }
        Ok(())
    })();
    #[cfg(feature = "import-baseline")]
    observation.operation(result.as_ref().ok().map(|_| false));
    #[cfg(feature = "import-baseline")]
    lifecycle.operation(match &result {
        Ok(_) => temporary::Outcome::Published,
        Err(_) => temporary::Outcome::Failed,
    });
    let remains = temporary.exists();
    #[cfg(feature = "import-baseline")]
    lifecycle.cleanup_checked(remains);
    if remains {
        #[cfg(feature = "import-baseline")]
        observation.cleanup_started();
        #[cfg(feature = "import-baseline")]
        lifecycle.step(temporary::Action::Cleanup);
        let removed = fs::remove_file(&temporary);
        #[cfg(feature = "import-baseline")]
        lifecycle.result(removed.is_ok());
        let cleanup = removed.map_err(|source| Error::Io {
            operation: "retire typed store replacement file",
            path: temporary,
            source,
        });
        #[cfg(feature = "import-baseline")]
        observation.cleanup(cleanup.is_ok());
        #[cfg(feature = "import-baseline")]
        if let Some(ledger) = &ledger {
            ledger.cleanup(cleanup.is_ok());
        }
        cleanup?;
    }
    #[cfg(feature = "import-baseline")]
    if let Some(ledger) = &ledger {
        ledger.completed(result.is_ok());
    }
    #[cfg(feature = "import-baseline")]
    lifecycle.completed(match &result {
        Ok(_) => temporary::Outcome::Published,
        Err(_) => temporary::Outcome::Failed,
    });
    result
}

#[cfg(feature = "import-baseline")]
pub(crate) enum Retirement<'a> {
    Pin(&'a crate::metrics::principal::pin::Attempt),
    Allocation(&'a crate::metrics::principal::retirement::Attempt),
}

pub(crate) fn stage_retirement(
    publication: &super::lease::Publication<'_>,
    root: &Path,
    target: &Path,
    #[cfg(feature = "import-baseline")] observation: &mut crate::metrics::publication::Attempt,
    #[cfg(feature = "import-baseline")] retirement: Option<Retirement<'_>>,
) -> Result<PathBuf, Error> {
    let admitted = super::accounting::admit_control(
        publication,
        root,
        super::control::Plan::retirement(target),
    )?;
    stage_admitted_retirement(
        admitted,
        publication,
        root,
        target,
        #[cfg(feature = "import-baseline")]
        observation,
        #[cfg(feature = "import-baseline")]
        retirement,
    )
}

pub(crate) fn stage_admitted_retirement(
    admitted: super::control::Admitted<'_>,
    publication: &super::lease::Publication<'_>,
    root: &Path,
    target: &Path,
    #[cfg(feature = "import-baseline")] observation: &mut crate::metrics::publication::Attempt,
    #[cfg(feature = "import-baseline")] retirement: Option<Retirement<'_>>,
) -> Result<PathBuf, Error> {
    admitted.consume(publication, root, "retirement", target, 0)?;
    let process = std::process::id();
    let sequence = RESERVATION.fetch_add(1, Ordering::Relaxed);
    let temporary = root
        .join("temporaries")
        .join(format!("{process}-{sequence}"));
    fs::rename(target, &temporary).map_err(|source| Error::Io {
        operation: "stage whole typed store allocation retirement",
        path: target.to_path_buf(),
        source,
    })?;
    #[cfg(feature = "import-baseline")]
    observation.created();
    #[cfg(feature = "import-baseline")]
    if let Some(retirement) = retirement {
        match retirement {
            Retirement::Pin(pin) => pin.relocated(publication, &temporary),
            Retirement::Allocation(attempt) => {
                attempt.relocated(publication, &temporary, process, sequence)
            }
        }
    }
    sync_directory(
        target
            .parent()
            .ok_or(Error::State("typed store retirement has no source parent"))?,
    )?;
    sync_directory(temporary.parent().ok_or(Error::State(
        "typed store retirement has no temporary parent",
    ))?)?;
    Ok(temporary)
}

pub(crate) fn sync_directory(path: &Path) -> Result<(), Error> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|source| Error::Io {
            operation: "sync typed store directory",
            path: path.to_path_buf(),
            source,
        })
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

//! Invocation binding retained across the consuming analysis callback.

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};

use crate::attempt_protocol;

use super::{attempt, join_path, Budget, Invocation};

pub(super) struct Prepared {
    directory: PathBuf,
    digest: String,
    rows: Vec<(String, String)>,
}

impl Prepared {
    pub(super) fn bind(
        invocations: &[Invocation],
        digest: &str,
        budget: &mut Budget,
    ) -> Result<Self, String> {
        let first = invocations
            .first()
            .ok_or("missing completion invocations")?;
        if invocations.len() > attempt_protocol::MAX_INVOCATIONS
            || invocations
                .iter()
                .any(|row| row.protocol_directory != first.protocol_directory)
        {
            return Err("inconsistent completion invocation population".to_owned());
        }
        attempt_protocol::identity(digest.as_bytes())?;
        for row in invocations {
            attempt_protocol::identity(row.invocation_nonce.as_bytes())?;
            attempt_protocol::identity(row.unit_correlation.as_bytes())?;
        }
        budget.path_capacity(first.protocol_directory.as_os_str().len())?;
        let directory = first.protocol_directory.clone();
        let digest = budget.owned(digest)?;
        budget.items(invocations.len())?;
        let mut rows = Vec::new();
        rows.try_reserve_exact(invocations.len())
            .map_err(|_| "completion binding allocation failed".to_owned())?;
        for invocation in invocations {
            rows.push((
                budget.owned(&invocation.invocation_nonce)?,
                budget.owned(&invocation.unit_correlation)?,
            ));
        }
        Ok(Self {
            directory,
            digest,
            rows,
        })
    }

    /// The caller must establish successful compiler exit and callback result
    /// before closing the collector and handing its result to this method.
    pub(super) fn publish(
        self,
        collected: attempt::Collected,
        budget: &mut Budget,
    ) -> Result<(), String> {
        self.write(
            collected.conservative,
            collected.displays(),
            collected.queries(),
            budget,
        )
    }

    fn write<'a>(
        self,
        conservative: bool,
        paths: impl ExactSizeIterator<Item = &'a str> + Clone,
        queries: impl ExactSizeIterator<Item = &'a str> + Clone,
        budget: &mut Budget,
    ) -> Result<(), String> {
        budget.items(self.rows.len())?;
        let mut rows = Vec::new();
        rows.try_reserve_exact(self.rows.len())
            .map_err(|_| "completion borrowed row allocation failed".to_owned())?;
        rows.extend(
            self.rows
                .iter()
                .map(|(nonce, correlation)| attempt_protocol::Invocation { nonce, correlation }),
        );
        let destination = join_path(&self.directory, Path::new("attempts"), budget)?;
        let name = budget.formatted(format_args!(".attempts.tmp-{}", std::process::id()))?;
        let temporary = join_path(&self.directory, Path::new(&name), budget)?;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
            .map_err(|error| format!("create compiler completion temporary: {error}"))?;
        let result = (|| {
            attempt_protocol::encode(&mut file, &self.digest, conservative, &rows, paths, queries)
                .map_err(|error| format!("encode compiler completion: {error}"))?;
            file.sync_all()
                .map_err(|error| format!("sync compiler completion: {error}"))?;
            fs::hard_link(&temporary, &destination)
                .map_err(|error| format!("publish compiler completion: {error}"))
        })();
        drop(file);
        let cleanup = fs::remove_file(&temporary)
            .map_err(|error| format!("retire compiler completion temporary: {error}"));
        result.and(cleanup)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const ID: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

    fn prepared(directory: &Path) -> Prepared {
        Prepared {
            directory: directory.to_owned(),
            digest: ID.to_owned(),
            rows: vec![(ID.to_owned(), ID.to_owned())],
        }
    }

    #[test]
    fn publishes_exact_record_without_replacing_existing_completion() {
        let directory =
            std::env::temp_dir().join(format!("rawr-completion-test-{}", std::process::id()));
        fs::create_dir(&directory).unwrap();
        let mut budget = Budget::standard();
        prepared(&directory)
            .write(false, ["/a.rlib"].into_iter(), [].into_iter(), &mut budget)
            .unwrap();
        let original = fs::read(directory.join("attempts")).unwrap();
        let decoded = attempt_protocol::Completion::decode(&original).unwrap();
        assert_eq!(decoded.paths().collect::<Vec<_>>(), ["/a.rlib"]);
        assert!(prepared(&directory)
            .write(true, [].into_iter(), [].into_iter(), &mut budget)
            .is_err());
        assert_eq!(fs::read(directory.join("attempts")).unwrap(), original);
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 1);
        fs::remove_dir_all(directory).unwrap();
    }
}

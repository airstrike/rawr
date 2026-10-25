use std::fmt;
use std::io::Write as _;
use std::path::{Component, Path};

use super::{index, Account, Buffer, Legacy};
use crate::cargo::{universe, Limits};
use crate::input::build::Evidence;
use crate::input::proof::{Canonical, ProtectedEntry, TreeDirectory, TreeFile};
use crate::{Error, Result};

impl Account for Legacy<'_> {
    type Error = Error;
    type Index<K: Ord, V: Copy> = index::LegacyIndex<K, V>;

    fn check(&mut self) -> Result<()> {
        Ok(())
    }
    fn comparison(&mut self, bytes: usize) -> Result<()> {
        self.0.check()?;
        self.1.decode(bytes).map_err(|error| {
            self.0.refused = true;
            Error::Input {
                code: "protected-build-comparison-work",
                reason: error.to_string().into_boxed_str(),
            }
        })
    }
    fn hash_work(&mut self, bytes: usize) -> Result<()> {
        self.0.check()?;
        self.1.hash(bytes).map_err(|error| {
            self.0.refused = true;
            Error::Input {
                code: "protected-build-hash-work",
                reason: error.to_string().into_boxed_str(),
            }
        })
    }
    fn policy(&mut self, code: &'static str, reason: impl fmt::Display) -> Error {
        crate::input::input(code, reason)
    }
    /// Every `limited` comparison in the protected projection is an
    /// accounting target; record a crossing on the input account and continue.
    fn limited(&mut self, resource: &'static str, limit: usize, actual: usize) -> Result<()> {
        self.0.check()?;
        self.0.exceeded(resource, limit, actual);
        Ok(())
    }
    fn join(&mut self) -> Result<()> {
        self.0.items(1)
    }
    fn roots<T>(&mut self, count: usize) -> Result<Vec<T>> {
        self.0.items(count)?;
        let mut roots = Vec::new();
        roots
            .try_reserve_exact(count)
            .map_err(|error| Error::Input {
                code: "protected-build-owner-index-allocation",
                reason: error.to_string().into_boxed_str(),
            })?;
        Ok(roots)
    }
    fn push(&mut self, entries: &mut Buffer<ProtectedEntry>, value: ProtectedEntry) -> Result<()> {
        self.0.items(1)?;
        entries
            .values
            .try_reserve(1)
            .map_err(|error| Error::Input {
                code: "protected-build-allocation",
                reason: error.to_string().into_boxed_str(),
            })?;
        entries.values.push(value);
        Ok(())
    }
    fn token(&mut self, protected: &universe::MetadataProof, id: &str) -> Result<Option<String>> {
        Ok(protected.package_token(id))
    }
    fn normalized(
        &mut self,
        value: &str,
        evidence: &Evidence,
        protected: &universe::MetadataProof,
    ) -> Result<String> {
        let value = protected.normalized_text(value);
        let outputs = evidence
            .scripts
            .iter()
            .filter_map(|script| Some((script.out_dir.to_str()?, &*script.package_id)))
            .collect::<Vec<_>>();
        Ok(replace_outputs(value, outputs, |package| {
            protected.package_token(package)
        }))
    }
    fn path(&mut self, owner: &str, relative: &Path, source: &Path) -> Result<String> {
        let mut normalized = format!("$PACKAGE/{owner}");
        for component in relative.components() {
            let Component::Normal(component) = component else {
                return Err(crate::input::input(
                    "protected-build-package-path-shape",
                    source.display(),
                ));
            };
            normalized.push('/');
            normalized.push_str(component.to_str().ok_or_else(|| {
                crate::input::input("protected-build-package-path-encoding", source.display())
            })?);
        }
        Ok(normalized)
    }
    fn format(&mut self, arguments: fmt::Arguments<'_>) -> Result<String> {
        Ok(arguments.to_string())
    }
    fn boxed(&mut self, value: String) -> Result<Box<str>> {
        Ok(value.into_boxed_str())
    }
    fn text(&mut self, value: &str) -> Result<Box<str>> {
        Ok(value.into())
    }
    fn strings(&mut self, value: &[Box<str>]) -> Result<Box<[Box<str>]>> {
        Ok(value.into())
    }
    fn bytes(&mut self, value: &[u8]) -> Result<Box<[u8]>> {
        Ok(value.into())
    }
    fn directories(&mut self, value: &[TreeDirectory]) -> Result<Box<[TreeDirectory]>> {
        Ok(value.into())
    }
    fn files(&mut self, value: &[TreeFile]) -> Result<Box<[TreeFile]>> {
        Ok(value.into())
    }
    fn canonical(
        &mut self,
        entries: &[ProtectedEntry],
        limits: &Limits,
    ) -> Result<universe::BuildProjection> {
        // The projection size is an accounting target recorded on the
        // input account; the buffer itself is bounded by `try_reserve`.
        let mut canonical = Canonical { bytes: Vec::new() };
        let encoded = serde_json::to_writer(
            &mut canonical,
            &(universe::BUILD_PROJECTION_SCHEMA, entries),
        );
        encoded.map_err(|error| Error::Input {
            code: "protected-build-encoding",
            reason: error.to_string().into_boxed_str(),
        })?;
        canonical.write_all(b"\n").map_err(|error| Error::Input {
            code: "protected-build-encoding",
            reason: error.to_string().into_boxed_str(),
        })?;
        self.0.exceeded(
            "protected source/build projection bytes",
            limits.input_proof_bytes,
            canonical.bytes.len(),
        );
        self.0.bytes(canonical.bytes.len())?;
        Ok(universe::BuildProjection::from_canonical(
            canonical.bytes.into_boxed_slice(),
        ))
    }
}

/// Replace every build-script output directory in `value` by its token, the
/// longest directory first. Every directory shares the build directory's
/// prefix: a value without it names none of them and needs no per-directory
/// search.
fn replace_outputs(
    mut value: String,
    outputs: Vec<(&str, &str)>,
    token: impl Fn(&str) -> Option<String>,
) -> String {
    let shared = outputs
        .iter()
        .map(|(path, _)| *path)
        .reduce(|shared, path| {
            let length = shared
                .bytes()
                .zip(path.bytes())
                .take_while(|(left, right)| left == right)
                .count();
            &shared[..shared.floor_char_boundary(length)]
        });
    if shared.is_none_or(|shared| !value.contains(shared)) {
        return value;
    }
    let Some(shared) = shared.filter(|shared| !shared.is_empty()) else {
        let mut outputs = outputs
            .into_iter()
            .filter_map(|(path, package)| Some((path, token(package)?)))
            .collect::<Vec<_>>();
        outputs.sort_unstable_by_key(|(path, _)| std::cmp::Reverse(path.len()));
        for (path, token) in outputs {
            value = value.replace(path, &format!("$OUT/{token}"));
        }
        return value;
    };
    // Every directory starts with the shared prefix: only its occurrences can
    // begin one, so each is matched there, longest directory first.
    let mut outputs = outputs
        .into_iter()
        .filter_map(|(path, package)| Some((path, token(package)?)))
        .collect::<Vec<_>>();
    outputs.sort_unstable_by_key(|(path, _)| std::cmp::Reverse(path.len()));
    let mut replaced = String::with_capacity(value.len());
    let mut rest = value.as_str();
    while let Some(index) = rest.find(shared) {
        replaced.push_str(&rest[..index]);
        rest = &rest[index..];
        match outputs.iter().find(|(path, _)| rest.starts_with(path)) {
            Some((path, token)) => {
                replaced.push_str("$OUT/");
                replaced.push_str(token);
                rest = &rest[path.len()..];
            }
            None => {
                let step = rest.chars().next().map_or(1, char::len_utf8);
                replaced.push_str(&rest[..step]);
                rest = &rest[step..];
            }
        }
    }
    replaced.push_str(rest);
    replaced
}

#[cfg(test)]
mod output_tests {
    #[test]
    fn output_directories_are_replaced_longest_first_and_other_values_are_kept() {
        let outputs = || {
            vec![
                ("/t/build/a-1/out", "a"),
                ("/t/build/a-1/out/nested", "n"),
                ("/t/build/b-2/out", "b"),
                ("/t/build/c-3/out", "untokened"),
            ]
        };
        let token = |package: &str| (package != "untokened").then(|| package.to_owned());
        assert_eq!(
            super::replace_outputs(
                "-L /t/build/a-1/out/nested -L /t/build/b-2/out -L /t/build/c-3/out".into(),
                outputs(),
                token
            ),
            "-L $OUT/n -L $OUT/b -L /t/build/c-3/out"
        );
        assert_eq!(
            super::replace_outputs("/t/source/lib.rs".into(), outputs(), token),
            "/t/source/lib.rs"
        );
        let none = super::replace_outputs("/t/build/a-1/out".into(), Vec::new(), token);
        assert_eq!(none, "/t/build/a-1/out");
    }
}

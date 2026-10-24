//! Complete off-side snapshot generations and comparison outcomes.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use super::{artifact, headless, image, inventory, manifest, Class, Commit, Error, Notice};

pub(super) struct Generation {
    artifacts: Vec<artifact::Artifact>,
    manifest: manifest::Manifest,
}

/// One exact mismatch between a committed baseline and fresh actual artifact.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Difference {
    identity: String,
    theme: String,
    class: Class,
    expected: PathBuf,
    actual: PathBuf,
    detail: String,
}

/// Result of checking or updating one complete snapshot generation.
#[derive(Debug)]
pub struct Outcome {
    baseline: PathBuf,
    actual: PathBuf,
    artifacts: usize,
    differences: Vec<Difference>,
    updated: bool,
    commit: Commit,
}

impl Generation {
    pub(super) fn render(
        catalog: &crate::preview::catalog::Authority,
        renderer: &mut headless::Renderer,
        selection: Option<&super::Selection>,
    ) -> Result<Self, Error> {
        let mut artifacts = Vec::new();
        let mut paths = BTreeMap::new();
        for definition in catalog.previews().iter().filter(|definition| {
            selection.map_or(definition.snapshot(), |selection| {
                selection.preview(definition)
            })
        }) {
            for theme in artifact::themes(definition)
                .into_iter()
                .filter(|theme| selection.is_none_or(|selection| selection.theme(theme)))
            {
                let artifact = artifact::Artifact::render(catalog, definition, theme, renderer)?;
                reserve(
                    &mut paths,
                    artifact.relative(),
                    &format!("{} ({})", artifact.identity(), artifact.theme().label()),
                )?;
                artifacts.push(artifact);
            }
        }
        if artifacts.is_empty() {
            return Err(Error::Empty);
        }
        let manifest = manifest::Manifest::new(&artifacts);
        Ok(Self {
            artifacts,
            manifest,
        })
    }

    pub(super) fn write(&self, root: &Path) -> Result<(), Error> {
        std::fs::create_dir_all(root).map_err(|source| Error::io(root, source))?;
        for artifact in &self.artifacts {
            let path = root.join(artifact.relative());
            if !path.starts_with(root) {
                return Err(Error::UnsafePath { path });
            }
            artifact.image().write(&path)?;
        }
        self.manifest.write(root)
    }

    pub(super) fn compare(
        &self,
        baseline: &Path,
        actual: &Path,
        prior: Option<&manifest::Manifest>,
        inventory: &inventory::Inventory,
    ) -> Result<Vec<Difference>, Error> {
        let current = self.manifest.by_key();
        let previous = prior.map(manifest::Manifest::by_key).unwrap_or_default();
        let mut differences = Vec::new();

        for artifact in &self.artifacts {
            let key = (
                artifact.identity(),
                artifact.theme().kind(),
                artifact.theme().name(),
            );
            let actual_path = actual.join(artifact.relative());
            let Some(expected) = previous.get(&key).copied() else {
                differences.push(Difference::new(
                    artifact.identity(),
                    artifact.theme().label(),
                    Class::Missing,
                    baseline.join(artifact.relative()),
                    actual_path,
                    "no manifest entry exists for this rendered artifact",
                ));
                continue;
            };
            let expected_path = baseline.join(expected.relative());
            let Some(expected_bytes) = inventory.bytes(expected.relative()) else {
                differences.push(Difference::new(
                    artifact.identity(),
                    artifact.theme().label(),
                    Class::Missing,
                    expected_path,
                    actual_path,
                    "manifest entry exists but its baseline PNG is missing",
                ));
                continue;
            };
            let expected_image =
                image::Image::decode(&expected_path, expected_bytes).map_err(|source| {
                    image_error(
                        artifact.identity(),
                        artifact.theme().label(),
                        &expected_path,
                        &actual_path,
                        source,
                    )
                })?;
            if expected_image.width() != artifact.image().width()
                || expected_image.height() != artifact.image().height()
            {
                differences.push(Difference::new(
                    artifact.identity(),
                    artifact.theme().label(),
                    Class::Size,
                    expected_path,
                    actual_path,
                    format!(
                        "baseline PNG is {}x{}; actual is {}x{}",
                        expected_image.width(),
                        expected_image.height(),
                        artifact.image().width(),
                        artifact.image().height(),
                    ),
                ));
                continue;
            }
            if let Some((x, y, expected, got)) = first_difference(
                expected_image.bytes(),
                artifact.image().bytes(),
                artifact.image().width(),
            ) {
                differences.push(Difference::new(
                    artifact.identity(),
                    artifact.theme().label(),
                    Class::Pixel,
                    expected_path,
                    actual_path,
                    format!(
                        "first changed pixel ({x}, {y}): baseline {expected:?}, actual {got:?}"
                    ),
                ));
            }
        }

        for (key, entry) in previous {
            if !current.contains_key(&key) {
                differences.push(Difference::new(
                    entry.identity(),
                    entry.theme(),
                    Class::Stale,
                    baseline.join(entry.relative()),
                    actual.join(entry.relative()),
                    "the prior manifest entry is not part of the fresh generation",
                ));
            }
        }

        let claimed = prior
            .map(|manifest| {
                manifest
                    .entries()
                    .iter()
                    .map(|entry| entry.relative().to_path_buf())
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        for file in inventory.paths() {
            if file != Path::new(manifest::FILE) && !claimed.contains(file) {
                differences.push(Difference::new(
                    format!("unclaimed artifact `{}`", file.display()),
                    "unknown",
                    Class::Stale,
                    baseline.join(file),
                    actual.join(file),
                    "the baseline tree contains a file absent from its manifest",
                ));
            }
        }
        Ok(differences)
    }

    pub(super) fn len(&self) -> usize {
        self.artifacts.len()
    }
}

fn reserve(paths: &mut BTreeMap<String, String>, path: &Path, identity: &str) -> Result<(), Error> {
    let canonical = path.to_string_lossy().to_ascii_lowercase();
    if let Some(first) = paths.insert(canonical, identity.to_owned()) {
        return Err(Error::Collision {
            path: path.to_path_buf(),
            first,
            second: identity.to_owned(),
        });
    }
    Ok(())
}

fn image_error(
    identity: &str,
    theme: String,
    expected: &Path,
    actual: &Path,
    source: Error,
) -> Error {
    Error::Image {
        identity: identity.to_owned(),
        theme,
        expected: expected.to_path_buf(),
        actual: actual.to_path_buf(),
        source: Box::new(source),
    }
}

impl Difference {
    pub(super) fn new(
        identity: impl Into<String>,
        theme: impl Into<String>,
        class: Class,
        expected: PathBuf,
        actual: PathBuf,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            identity: identity.into(),
            theme: theme.into(),
            class,
            expected,
            actual,
            detail: detail.into(),
        }
    }

    /// Return the stable preview identity.
    #[must_use]
    pub fn identity(&self) -> &str {
        &self.identity
    }

    /// Return the deterministic theme input.
    #[must_use]
    pub fn theme(&self) -> &str {
        &self.theme
    }

    /// Return the mismatch class.
    #[must_use]
    pub const fn class(&self) -> Class {
        self.class
    }

    /// Return the committed expected artifact path.
    #[must_use]
    pub fn expected(&self) -> &Path {
        &self.expected
    }

    /// Return the freshly rendered actual artifact path.
    #[must_use]
    pub fn actual(&self) -> &Path {
        &self.actual
    }

    /// Return precise mismatch evidence.
    #[must_use]
    pub fn detail(&self) -> &str {
        &self.detail
    }
}

impl Outcome {
    pub(super) fn new(
        baseline: PathBuf,
        actual: PathBuf,
        artifacts: usize,
        differences: Vec<Difference>,
        updated: bool,
        commit: Commit,
    ) -> Self {
        Self {
            baseline,
            actual,
            artifacts,
            differences,
            updated,
            commit,
        }
    }

    /// Return the committed baseline root.
    #[must_use]
    pub fn baseline(&self) -> &Path {
        &self.baseline
    }

    /// Return the disposable actual-artifact root.
    #[must_use]
    pub fn actual(&self) -> &Path {
        &self.actual
    }

    /// Return the number of deterministic artifacts rendered.
    #[must_use]
    pub const fn artifacts(&self) -> usize {
        self.artifacts
    }

    /// Return every exact mismatch in stable order.
    #[must_use]
    pub fn differences(&self) -> &[Difference] {
        &self.differences
    }

    /// Return cleanup warnings retained after artifact commit points.
    #[must_use]
    pub fn notices(&self) -> &[Notice] {
        self.commit.notices()
    }

    /// Return whether this operation promoted a complete baseline generation.
    #[must_use]
    pub const fn updated(&self) -> bool {
        self.updated
    }

    /// Return whether every rendered artifact matches its baseline.
    #[must_use]
    pub fn is_successful(&self) -> bool {
        self.updated || self.differences.is_empty()
    }
}

fn first_difference(
    expected: &[u8],
    actual: &[u8],
    width: u32,
) -> Option<(u32, u32, [u8; 4], [u8; 4])> {
    let byte = expected
        .iter()
        .zip(actual)
        .position(|(expected, actual)| expected != actual)?;
    let pixel = byte / 4;
    let start = pixel * 4;
    let mut expected_pixel = [0; 4];
    let mut actual_pixel = [0; 4];
    expected_pixel.copy_from_slice(&expected[start..start + 4]);
    actual_pixel.copy_from_slice(&actual[start..start + 4]);
    Some((
        (pixel as u32) % width,
        (pixel as u32) / width,
        expected_pixel,
        actual_pixel,
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::Path;

    #[test]
    fn pixel_difference_reports_the_complete_first_changed_pixel() {
        let expected = [1, 2, 3, 4, 5, 6, 7, 8];
        let actual = [1, 2, 3, 4, 5, 9, 7, 8];
        assert_eq!(
            super::first_difference(&expected, &actual, 2),
            Some((1, 0, [5, 6, 7, 8], [5, 9, 7, 8]))
        );
    }

    #[test]
    fn image_io_source_retains_complete_comparison_context() {
        let expected = Path::new("snapshots/main/view/default/native.png");
        let actual = Path::new("target/rawr/snapshots/main/view/default/native.png");
        let source = super::Error::io(
            expected,
            std::io::Error::new(std::io::ErrorKind::InvalidData, "corrupt PNG stream"),
        );
        let error = super::image_error(
            "main::view#default",
            "native".to_owned(),
            expected,
            actual,
            source,
        );

        let super::Error::Image {
            identity,
            theme,
            expected: retained_expected,
            actual: retained_actual,
            source,
        } = error
        else {
            panic!("expected contextual image error");
        };
        assert_eq!(identity, "main::view#default");
        assert_eq!(theme, "native");
        assert_eq!(retained_expected, expected);
        assert_eq!(retained_actual, actual);
        assert!(matches!(*source, super::Error::Io { .. }));
    }

    #[test]
    fn reservation_rejects_case_insensitive_whole_path_collisions() {
        let mut paths = BTreeMap::new();
        super::reserve(
            &mut paths,
            Path::new("Counter-a81f42c0/view/default/Dark.png"),
            "first",
        )
        .unwrap();
        let error = super::reserve(
            &mut paths,
            Path::new("counter-A81F42C0/VIEW/default/dark.PNG"),
            "second",
        )
        .unwrap_err();

        assert!(matches!(
            error,
            super::Error::Collision { first, second, .. }
                if first == "first" && second == "second"
        ));
    }
}

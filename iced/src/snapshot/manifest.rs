//! Versioned deterministic evidence for one complete generation.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use super::{artifact, encoding, headless, Error};

pub(super) const FILE: &str = "manifest.rawr-snapshot";
const HEADER: &str = "rawr-snapshot\t1";

#[derive(Clone, Debug)]
pub(super) struct Manifest {
    facts: Facts,
    entries: Vec<Entry>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Entry {
    identity: String,
    fingerprint: String,
    theme_kind: String,
    theme_name: String,
    logical: (u32, u32),
    physical: (u32, u32),
    relative: PathBuf,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct Facts {
    rawr: String,
    adapter: String,
    iced: String,
    renderer: String,
    font: String,
    scale: String,
}

impl Manifest {
    pub(super) fn new(artifacts: &[artifact::Artifact]) -> Self {
        let mut entries = artifacts.iter().map(Entry::from).collect::<Vec<_>>();
        entries.sort_by(|left, right| left.order().cmp(&right.order()));
        Self {
            facts: Facts::current(),
            entries,
        }
    }

    pub(super) fn write(&self, root: &Path) -> Result<(), Error> {
        let path = root.join(FILE);
        fs::write(&path, self.text()).map_err(|source| Error::io(path, source))
    }

    pub(super) fn read(path: &Path, bytes: Option<&[u8]>) -> Result<Option<Self>, Error> {
        let Some(bytes) = bytes else {
            return Ok(None);
        };
        let text = std::str::from_utf8(bytes)
            .map_err(|_| invalid(path, 1, "manifest must be valid UTF-8"))?;
        Self::parse(path, text).map(Some)
    }

    pub(super) fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub(super) fn by_key(&self) -> BTreeMap<(&str, &str, &str), &Entry> {
        self.entries
            .iter()
            .map(|entry| (entry.key(), entry))
            .collect()
    }

    fn text(&self) -> String {
        let mut text = String::new();
        text.push_str(HEADER);
        text.push('\n');
        for (name, value) in self.facts.rows() {
            text.push_str(name);
            text.push('\t');
            text.push_str(&encoding::field(value));
            text.push('\n');
        }
        for entry in &self.entries {
            text.push_str("artifact");
            for field in entry.fields() {
                text.push('\t');
                text.push_str(&encoding::field(&field));
            }
            text.push('\n');
        }
        text
    }

    fn parse(path: &Path, text: &str) -> Result<Self, Error> {
        let mut lines = text.lines();
        if lines.next() != Some(HEADER) {
            return Err(invalid(path, 1, "unsupported or missing manifest header"));
        }
        let mut rows = Vec::new();
        for line in 2..=7 {
            let Some(row) = lines.next() else {
                return Err(invalid(path, line, "missing build fact"));
            };
            let mut fields = row.split('\t');
            let name = fields.next().unwrap_or_default();
            let value = fields
                .next()
                .ok_or_else(|| invalid(path, line, "missing build-fact value"))?;
            if fields.next().is_some() {
                return Err(invalid(path, line, "unexpected build-fact field"));
            }
            rows.push((name.to_owned(), decode(path, line, value)?));
        }
        let facts = Facts::from_rows(path, rows)?;
        let mut entries = Vec::new();
        for (offset, row) in lines.enumerate() {
            let line = offset + 8;
            let fields = row.split('\t').collect::<Vec<_>>();
            if fields.len() != 10 || fields[0] != "artifact" {
                return Err(invalid(path, line, "expected ten-field artifact row"));
            }
            let values = fields[1..]
                .iter()
                .map(|field| decode(path, line, field))
                .collect::<Result<Vec<_>, _>>()?;
            entries.push(Entry::parse(path, line, &values)?);
        }
        let unique = entries
            .iter()
            .map(Entry::key)
            .collect::<std::collections::BTreeSet<_>>();
        if unique.len() != entries.len() {
            return Err(invalid(path, 8, "artifact identities must be unique"));
        }
        let paths = entries
            .iter()
            .map(Entry::relative)
            .collect::<std::collections::BTreeSet<_>>();
        if paths.len() != entries.len() {
            return Err(invalid(path, 8, "artifact paths must be unique"));
        }
        entries.sort_by(|left, right| left.order().cmp(&right.order()));
        let manifest = Self { facts, entries };
        let canonical = manifest.text();
        if canonical != text {
            return Err(invalid(
                path,
                first_difference(text, &canonical),
                "manifest order or escaping is not canonical",
            ));
        }
        Ok(manifest)
    }
}

impl Entry {
    pub(super) fn key(&self) -> (&str, &str, &str) {
        (&self.identity, &self.theme_kind, &self.theme_name)
    }

    pub(super) fn identity(&self) -> &str {
        &self.identity
    }

    pub(super) fn theme(&self) -> String {
        format!("{} `{}`", self.theme_kind, self.theme_name)
    }

    pub(super) fn relative(&self) -> &Path {
        &self.relative
    }

    fn order(&self) -> (&str, u8, &Path) {
        (
            &self.identity,
            u8::from(self.theme_kind != "native"),
            &self.relative,
        )
    }

    fn fields(&self) -> [String; 9] {
        [
            self.identity.clone(),
            self.fingerprint.clone(),
            self.theme_kind.clone(),
            self.theme_name.clone(),
            self.logical.0.to_string(),
            self.logical.1.to_string(),
            self.physical.0.to_string(),
            self.physical.1.to_string(),
            relative(&self.relative),
        ]
    }

    fn parse(path: &Path, line: usize, values: &[String]) -> Result<Self, Error> {
        let number = |index: usize| {
            values[index]
                .parse::<u32>()
                .map_err(|_| invalid(path, line, "image dimensions must be u32 integers"))
        };
        let relative = PathBuf::from(&values[8]);
        safe(path, line, &relative)?;
        Ok(Self {
            identity: values[0].clone(),
            fingerprint: values[1].clone(),
            theme_kind: values[2].clone(),
            theme_name: values[3].clone(),
            logical: (number(4)?, number(5)?),
            physical: (number(6)?, number(7)?),
            relative,
        })
    }
}

impl From<&artifact::Artifact> for Entry {
    fn from(artifact: &artifact::Artifact) -> Self {
        Self {
            identity: artifact.identity().to_owned(),
            fingerprint: artifact.fingerprint().to_owned(),
            theme_kind: artifact.theme().kind().to_owned(),
            theme_name: artifact.theme().name().to_owned(),
            logical: (artifact.logical().width(), artifact.logical().height()),
            physical: (artifact.image().width(), artifact.image().height()),
            relative: artifact.relative().to_path_buf(),
        }
    }
}

impl Facts {
    fn current() -> Self {
        Self {
            rawr: rawr::VERSION.to_owned(),
            adapter: env!("CARGO_PKG_VERSION").to_owned(),
            iced: headless::ICED_REVISION.to_owned(),
            renderer: headless::BACKEND.to_owned(),
            font: headless::FONT.to_owned(),
            scale: "1.0".to_owned(),
        }
    }

    fn rows(&self) -> [(&'static str, &str); 6] {
        [
            ("rawr", &self.rawr),
            ("adapter", &self.adapter),
            ("iced", &self.iced),
            ("renderer", &self.renderer),
            ("font", &self.font),
            ("scale", &self.scale),
        ]
    }

    fn from_rows(path: &Path, rows: Vec<(String, String)>) -> Result<Self, Error> {
        let expected = ["rawr", "adapter", "iced", "renderer", "font", "scale"];
        if !rows.iter().map(|(name, _)| name.as_str()).eq(expected) {
            return Err(invalid(path, 2, "build facts are missing or out of order"));
        }
        let mut values = rows.into_iter().map(|(_, value)| value);
        let mut next = || {
            values
                .next()
                .ok_or_else(|| invalid(path, 2, "build facts are missing or out of order"))
        };
        Ok(Self {
            rawr: next()?,
            adapter: next()?,
            iced: next()?,
            renderer: next()?,
            font: next()?,
            scale: next()?,
        })
    }
}

fn relative(path: &Path) -> String {
    path.components()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn safe(manifest: &Path, line: usize, path: &Path) -> Result<(), Error> {
    if path.components().next().is_none()
        || path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(invalid(
            manifest,
            line,
            "artifact path must be relative and normalized",
        ));
    }
    Ok(())
}

fn decode(path: &Path, line: usize, value: &str) -> Result<String, Error> {
    encoding::decode(value).map_err(|message| invalid(path, line, message))
}

fn invalid(path: &Path, line: usize, message: impl Into<String>) -> Error {
    Error::Manifest {
        path: path.to_path_buf(),
        line,
        message: message.into(),
    }
}

fn first_difference(actual: &str, canonical: &str) -> usize {
    let byte = actual
        .bytes()
        .zip(canonical.bytes())
        .position(|(actual, canonical)| actual != canonical)
        .unwrap_or_else(|| actual.len().min(canonical.len()));
    actual.as_bytes()[..byte.min(actual.len())]
        .iter()
        .filter(|byte| **byte == b'\n')
        .count()
        + 1
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{Entry, Facts, Manifest};

    #[test]
    fn parser_rejects_reordered_entries_and_noncanonical_escapes() {
        let path = Path::new("snapshots/manifest.rawr-snapshot");
        let canonical = manifest().text();
        Manifest::parse(path, &canonical).unwrap();

        let mut lines = canonical.lines().collect::<Vec<_>>();
        lines.swap(7, 8);
        let reordered = format!("{}\n", lines.join("\n"));
        assert!(Manifest::parse(path, &reordered)
            .unwrap_err()
            .to_string()
            .contains("not canonical"));

        let lowercase = canonical.replace("%5A", "%5a");
        assert_ne!(lowercase, canonical);
        assert!(Manifest::parse(path, &lowercase)
            .unwrap_err()
            .to_string()
            .contains("not canonical"));

        let unnecessary = canonical.replace("default", "%64efault");
        assert_ne!(unnecessary, canonical);
        assert!(Manifest::parse(path, &unnecessary)
            .unwrap_err()
            .to_string()
            .contains("not canonical"));
    }

    #[test]
    fn parser_rejects_shared_and_empty_artifact_paths() {
        let path = Path::new("snapshots/manifest.rawr-snapshot");
        let mut shared = manifest();
        let mut duplicate = shared.entries[0].clone();
        duplicate.identity = "Alpha#default".to_owned();
        shared.entries.insert(0, duplicate);

        let error = Manifest::parse(path, &shared.text()).unwrap_err();
        let crate::snapshot::Error::Manifest {
            path: retained,
            line,
            message,
        } = error
        else {
            panic!("expected typed manifest error, got {error}");
        };
        assert_eq!(retained, path);
        assert_eq!(line, 8);
        assert_eq!(message, "artifact paths must be unique");

        let mut empty = manifest();
        empty.entries[0].relative = PathBuf::new();
        let error = Manifest::parse(path, &empty.text()).unwrap_err();
        let crate::snapshot::Error::Manifest {
            path: retained,
            line,
            message,
        } = error
        else {
            panic!("expected typed manifest error, got {error}");
        };
        assert_eq!(retained, path);
        assert_eq!(line, 8);
        assert_eq!(message, "artifact path must be relative and normalized");
    }

    fn manifest() -> Manifest {
        let entry = |identity: &str, theme_kind: &str, relative: &str| Entry {
            identity: identity.to_owned(),
            fingerprint: "0123456789abcdef".to_owned(),
            theme_kind: theme_kind.to_owned(),
            theme_name: if theme_kind == "native" {
                "iced::Theme::Light".to_owned()
            } else {
                "dark".to_owned()
            },
            logical: (96, 64),
            physical: (96, 64),
            relative: PathBuf::from(relative),
        };
        Manifest {
            facts: Facts::current(),
            entries: vec![
                entry("Zed#default", "native", "zed/default/native.png"),
                entry("Zed#default", "explicit", "zed/default/theme-000-dark.png"),
            ],
        }
    }
}

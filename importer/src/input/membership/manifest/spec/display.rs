//! Cargo 59800466 PackageIdSpec::fmt and SourceKind::PrettyRef, without heap
//! allocation during formatting. A prepared view admits all input traversal.
// Copyright The Rust Project Developers. MIT OR Apache-2.0.
// See PROVENANCE.md and the retained reference Cargo licenses.
use super::{query, session, Kind, Spec, Step};
use rawr_toml::document;
use std::fmt;

pub(in super::super) struct Display<'a> {
    spec: &'a Spec,
    url: Option<&'a str>,
    print_name: bool,
}

impl Spec {
    pub(in super::super) fn display<A: document::Admission>(
        &self,
        session: &mut session::Session<'_, A>,
    ) -> Step<Display<'_>> {
        session.check()?;
        session.string(self.name.len())?;
        let (url, print_name) = match &self.url {
            Some(url) => (
                Some(url.as_str(session)?),
                url.last_path_segment(session)? != Some(self.name.as_str()),
            ),
            None => (None, true),
        };
        if let Some(Kind::Git(reference)) = &self.kind {
            session.string(reference.value().len())?;
        }
        if let Some(version) = &self.version {
            session.items(3)?;
            if let Some(pre) = version.pre() {
                session.string(pre.len())?;
            }
            if let Some(build) = version.build() {
                session.string(build.len())?;
            }
        }
        Ok(Display {
            spec: self,
            url,
            print_name,
        })
    }
}

impl fmt::Display for Display<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(url) = self.url {
            match &self.spec.kind {
                Some(Kind::Path) => f.write_str("path+")?,
                Some(Kind::Registry) => f.write_str("registry+")?,
                Some(Kind::Git(_)) => f.write_str("git+")?,
                Some(Kind::SparseRegistry) | None => {}
            }
            f.write_str(url)?;
            if let Some(Kind::Git(reference)) = &self.spec.kind {
                let key = match reference.kind() {
                    query::Kind::Tag => Some("?tag="),
                    query::Kind::Branch => Some("?branch="),
                    query::Kind::Rev => Some("?rev="),
                    query::Kind::DefaultBranch => None,
                };
                if let Some(key) = key {
                    f.write_str(key)?;
                    for byte in reference.value().bytes() {
                        match byte {
                            b' ' => f.write_str("+")?,
                            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'*' | b'-' | b'.' | b'_' => {
                                write!(f, "{}", char::from(byte))?
                            }
                            _ => write!(f, "%{byte:02X}")?,
                        }
                    }
                }
            }
            if self.print_name {
                write!(f, "#{}", self.spec.name)?;
            }
        } else {
            f.write_str(&self.spec.name)?;
        }
        if let Some(version) = &self.spec.version {
            write!(
                f,
                "{}{}",
                if self.print_name { '@' } else { '#' },
                version.major()
            )?;
            if let Some(minor) = version.minor() {
                write!(f, ".{minor}")?;
            }
            if let Some(patch) = version.patch() {
                write!(f, ".{patch}")?;
            }
            if let Some(pre) = version.pre() {
                write!(f, "-{pre}")?;
            }
            if let Some(build) = version.build() {
                write!(f, "+{build}")?;
            }
        }
        Ok(())
    }
}

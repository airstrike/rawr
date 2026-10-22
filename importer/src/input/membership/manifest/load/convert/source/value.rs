//! Owned source payloads; literal selectors never pass through URL query decoding.
use crate::input::membership::manifest::{load::formatted, scalar, session::Session, Step};
use rawr_toml::document;
use std::cmp::Ordering;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in super::super::super) enum Git<'a> {
    Tag(&'a str),
    Branch(&'a str),
    Rev(&'a str),
    DefaultBranch,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in super::super::super) enum Kind<'a> {
    Path,
    Registry,
    SparseRegistry,
    LocalRegistry,
    Directory,
    Git(Git<'a>),
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in super::super::super) enum Label<'a> {
    Registry(&'a str),
    Source(&'a str),
}
pub(in super::super::super) enum Precise<'a> {
    Locked,
    Updated {
        name: &'a str,
        from: scalar::Version<'a>,
        to: scalar::Version<'a>,
    },
    GitUrlFragment(&'a str),
}

pub(in super::super::super) enum Origin<'a> {
    Path,
    Registry,
    Git(Git<'a>),
}
pub(super) struct Owned {
    kind: OwnedKind,
    precise: Option<Precision>,
    label: Option<OwnedLabel>,
}
enum OwnedKind {
    Path,
    Registry,
    SparseRegistry,
    LocalRegistry,
    Directory,
    Git(OwnedGit),
}
enum OwnedGit {
    Tag(String),
    Branch(String),
    Rev(String),
    DefaultBranch,
}
enum OwnedLabel {
    Registry(String),
    Source(String),
}
enum Precision {
    Locked,
    Updated {
        name: String,
        from: Version,
        to: Version,
    },
    Git(String),
}
#[derive(Eq, PartialEq, Ord, PartialOrd)]
struct Version {
    major: u64,
    minor: u64,
    patch: u64,
    pre: String,
    build: String,
}
fn own<A: document::Admission>(value: &str, session: &mut Session<'_, A>) -> Step<String> {
    formatted(format_args!("{value}"), session)
}
impl Version {
    fn from<A: document::Admission>(
        value: scalar::Version<'_>,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.items(1)?;
        Ok(Self {
            major: value.major(),
            minor: value.minor(),
            patch: value.patch(),
            pre: own(value.pre(), session)?,
            build: own(value.build(), session)?,
        })
    }
    fn cmp_in<A: document::Admission>(
        &self,
        other: &Self,
        session: &mut Session<'_, A>,
    ) -> Step<Ordering> {
        session.string(self.pre.len())?;
        session.string(other.pre.len())?;
        session.string(self.build.len())?;
        session.string(other.build.len())?;
        session.items(self.pre.len())?;
        session.items(other.pre.len())?;
        session.items(self.build.len())?;
        session.items(other.build.len())?;
        // Private index order only: equality matches complete Version equality,
        // including build metadata. It is not public semver precedence.
        Ok(self.cmp(other))
    }
}
impl Owned {
    pub(super) fn from<A: document::Admission>(
        kind: Kind<'_>,
        precise: Option<Precise<'_>>,
        label: Option<Label<'_>>,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.items(1)?;
        let kind = match kind {
            Kind::Path => OwnedKind::Path,
            Kind::Registry => OwnedKind::Registry,
            Kind::SparseRegistry => OwnedKind::SparseRegistry,
            Kind::LocalRegistry => OwnedKind::LocalRegistry,
            Kind::Directory => OwnedKind::Directory,
            Kind::Git(reference) => OwnedKind::Git(match reference {
                Git::Tag(s) => OwnedGit::Tag(own(s, session)?),
                Git::Branch(s) => OwnedGit::Branch(own(s, session)?),
                Git::Rev(s) => OwnedGit::Rev(own(s, session)?),
                Git::DefaultBranch => OwnedGit::DefaultBranch,
            }),
        };
        let precise = match precise {
            None => None,
            Some(Precise::Locked) => Some(Precision::Locked),
            Some(Precise::GitUrlFragment(value)) => Some(Precision::Git(own(value, session)?)),
            Some(Precise::Updated { name, from, to }) => Some(Precision::Updated {
                name: own(name, session)?,
                from: Version::from(from, session)?,
                to: Version::from(to, session)?,
            }),
        };
        let label = match label {
            None => None,
            Some(Label::Registry(s)) => Some(OwnedLabel::Registry(own(s, session)?)),
            Some(Label::Source(s)) => Some(OwnedLabel::Source(own(s, session)?)),
        };
        Ok(Self {
            kind,
            precise,
            label,
        })
    }
    pub(super) fn kind(&self) -> Kind<'_> {
        match &self.kind {
            OwnedKind::Path => Kind::Path,
            OwnedKind::Registry => Kind::Registry,
            OwnedKind::SparseRegistry => Kind::SparseRegistry,
            OwnedKind::LocalRegistry => Kind::LocalRegistry,
            OwnedKind::Directory => Kind::Directory,
            OwnedKind::Git(git) => Kind::Git(match git {
                OwnedGit::Tag(s) => Git::Tag(s),
                OwnedGit::Branch(s) => Git::Branch(s),
                OwnedGit::Rev(s) => Git::Rev(s),
                OwnedGit::DefaultBranch => Git::DefaultBranch,
            }),
        }
    }
    pub(super) fn label(&self) -> Option<Label<'_>> {
        self.label.as_ref().map(|label| match label {
            OwnedLabel::Registry(s) => Label::Registry(s),
            OwnedLabel::Source(s) => Label::Source(s),
        })
    }
    pub(super) fn compare_kind<A: document::Admission>(
        &self,
        other: &Self,
        session: &mut Session<'_, A>,
    ) -> Step<Ordering> {
        fn key(kind: Kind<'_>) -> (u8, u8, Option<&str>) {
            match kind {
                Kind::Path => (0, 0, None),
                Kind::Registry => (1, 0, None),
                Kind::SparseRegistry => (2, 0, None),
                Kind::LocalRegistry => (3, 0, None),
                Kind::Directory => (4, 0, None),
                Kind::Git(git) => match git {
                    Git::Tag(s) => (5, 0, Some(s)),
                    Git::Branch(s) => (5, 1, Some(s)),
                    Git::Rev(s) => (5, 2, Some(s)),
                    Git::DefaultBranch => (5, 3, None),
                },
            }
        }
        let (a, ar, at) = key(self.kind());
        let (b, br, bt) = key(other.kind());
        let tags = (a, ar).cmp(&(b, br));
        if tags != Ordering::Equal {
            return Ok(tags);
        }
        match (at, bt) {
            (Some(left), Some(right)) => cmp_text(left, right, session),
            _ => Ok(Ordering::Equal),
        }
    }
    pub(super) fn compare_precise<A: document::Admission>(
        &self,
        other: &Self,
        session: &mut Session<'_, A>,
    ) -> Step<Ordering> {
        let tag = |p: &Option<Precision>| match p {
            None => 0,
            Some(Precision::Locked) => 1,
            Some(Precision::Updated { .. }) => 2,
            Some(Precision::Git(_)) => 3,
        };
        let tags = tag(&self.precise).cmp(&tag(&other.precise));
        if tags != Ordering::Equal {
            return Ok(tags);
        }
        match (&self.precise, &other.precise) {
            (Some(Precision::Git(a)), Some(Precision::Git(b))) => cmp_text(a, b, session),
            (
                Some(Precision::Updated {
                    name: an,
                    from: af,
                    to: at,
                }),
                Some(Precision::Updated {
                    name: bn,
                    from: bf,
                    to: bt,
                }),
            ) => {
                let name = cmp_text(an, bn, session)?;
                if name != Ordering::Equal {
                    return Ok(name);
                }
                let from = af.cmp_in(bf, session)?;
                if from != Ordering::Equal {
                    return Ok(from);
                }
                at.cmp_in(bt, session)
            }
            _ => Ok(Ordering::Equal),
        }
    }
}
fn cmp_text<A: document::Admission>(
    left: &str,
    right: &str,
    session: &mut Session<'_, A>,
) -> Step<Ordering> {
    session.string(left.len())?;
    session.string(right.len())?;
    session.items(left.len())?;
    session.items(right.len())?;
    Ok(left.cmp(right))
}

/// An owned literal Git selector at Cargo's pre-URL-parse clone point.
pub(in super::super::super) struct Selector(OwnedGit);
impl Selector {
    pub(in super::super::super) fn from<A: document::Admission>(
        git: Git<'_>,
        session: &mut Session<'_, A>,
    ) -> Step<Self> {
        session.items(1)?;
        Ok(Self(match git {
            Git::Tag(s) => OwnedGit::Tag(own(s, session)?),
            Git::Branch(s) => OwnedGit::Branch(own(s, session)?),
            Git::Rev(s) => OwnedGit::Rev(own(s, session)?),
            Git::DefaultBranch => OwnedGit::DefaultBranch,
        }))
    }
}
impl Owned {
    pub(super) fn git(selector: Selector) -> Self {
        Self {
            kind: OwnedKind::Git(selector.0),
            precise: None,
            label: None,
        }
    }
}

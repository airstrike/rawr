// Copyright the gitoxide contributors. MIT OR Apache-2.0.
//! Complete local Git configuration ownership. No repository authority.

use std::{borrow::Cow, path::PathBuf, sync::Arc};

use super::trust;
use crate::input::membership::manifest::{session::Session, Error, Step};
use rawr_toml::document::Admission;

mod file;
mod parse;
mod stage;
mod storage;
mod table;
mod value;
pub(super) use stage::{Failure, Stage};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Source {
    Local,
    Worktree,
}

#[derive(Debug)]
// gix-config's file metadata; read only through Debug.
#[allow(dead_code)]
struct Metadata {
    path: Option<PathBuf>,
    source: Source,
    level: u8,
    trust: trust::Level,
}

#[derive(Debug)]
struct Header<'a> {
    name: Name<'a>,
    separator: Option<Cow<'a, [u8]>>,
    subsection: Option<Cow<'a, [u8]>>,
}

#[derive(Debug)]
struct Name<'a>(Cow<'a, [u8]>);
impl PartialEq for Name<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.0.eq_ignore_ascii_case(&other.0)
    }
}
impl Eq for Name<'_> {}
impl std::hash::Hash for Name<'_> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for byte in self.0.iter() {
            std::hash::Hash::hash(&byte.to_ascii_lowercase(), state);
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
struct Id(usize);

#[derive(Debug)]
enum Event<'a> {
    Comment { tag: u8, text: Cow<'a, [u8]> },
    Header(Header<'a>),
    Name(Name<'a>),
    Separator,
    Value(Cow<'a, [u8]>),
    NotDone(Cow<'a, [u8]>),
    Done(Cow<'a, [u8]>),
    Newline(Cow<'a, [u8]>),
    Whitespace(Cow<'a, [u8]>),
}

type Front = smallvec::SmallVec<[Event<'static>; 8]>;

#[derive(Debug)]
struct Section {
    header: Header<'static>,
    body: storage::Vector<Event<'static>>,
    // gix-config's section metadata; read only through Debug.
    #[allow(dead_code)]
    meta: Arc<Metadata>,
    id: Id,
}

#[derive(Debug)]
enum Lookup {
    Terminal(storage::Vector<Id>),
    NonTerminal(table::Map<Cow<'static, [u8]>, storage::Vector<Id>>),
}

#[derive(Debug)]
pub(super) struct File {
    front: Front,
    post: table::Map<Id, Front>,
    lookup: table::Map<Name<'static>, storage::Vector<Lookup>>,
    sections: table::Map<Id, Section>,
    next: usize,
    order: storage::Order,
    // gix-config's file metadata; read only through Debug.
    #[allow(dead_code)]
    meta: Arc<Metadata>,
}

fn own<A: Admission>(bytes: &[u8], session: &mut Session<'_, A>) -> Step<Vec<u8>> {
    session.string(bytes.len())?;
    let mut output = storage::Vector::with_capacity(bytes.len(), session)?;
    output.extend_copy(bytes, session)?;
    Ok(output.into_values())
}

fn shared<A: Admission>(meta: Metadata, session: &mut Session<'_, A>) -> Step<Arc<Metadata>> {
    let Ok((layout, _)) = std::alloc::Layout::new::<[std::sync::atomic::AtomicUsize; 2]>()
        .extend(std::alloc::Layout::new::<Metadata>())
    else {
        return session.fail(Error::Overflow);
    };
    storage::allocation(layout.pad_to_align(), session)?;
    Ok(Arc::new(meta))
}

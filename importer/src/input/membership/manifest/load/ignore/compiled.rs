// Copyright the ripgrep contributors. MIT OR Unlicense.
//! ignore 0.4.26 gitignore.rs:338–352, borrowed builder to owned matcher.
use std::alloc::Layout;

use rawr_toml::document::{Admission, Request};

use super::{glob, paths, rule, Builder};
use crate::input::membership::manifest::{
    load::formatted, session::Session, storage::Buffer, Error, Step,
};

mod query;

#[cfg(test)]
mod build_tests;

/// Source field/drop order, with a separate exclusively owned lazy match Vec.
/// No Clone, shared scratch or guard escapes this execution owner.
pub(in super::super) struct Matcher {
    set: glob::compiled::Set,
    root: paths::Root,
    globs: Vec<rule::Prepared>,
    num_ignores: u64,
    num_whitelists: u64,
    matches: Option<Buffer<usize>>,
}

// ignore's build errors, rendered and charged as ignore renders them; nothing reports them yet.
#[allow(dead_code)]
pub(in super::super) enum Failure {
    /// Original outer ignore::Error::Glob, rendered at the build map_err site.
    Glob { glob: Option<String>, error: String },
    /// Kept distinct from returned regex errors; the unchanged fixed production
    /// account proves the mandatory source unwrap failure unreachable.
    Aho(glob::aho::Failure),
}

impl Matcher {
    pub(super) fn build<A: Admission>(
        builder: &Builder,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Self, Failure>> {
        let originals = builder.rules.originals.as_slice();
        // Source performs two complete filters, ignores first, before compiling.
        let num_ignores = count(originals, false, session)?;
        let num_whitelists = count(originals, true, session)?;
        let set = match glob::compiled::Set::build(builder.rules.patterns.as_slice(), session)? {
            Ok(set) => set,
            Err(glob::compiled::Failure::Aho(cause)) => return Ok(Err(Failure::Aho(cause))),
            Err(glob::compiled::Failure::Regex(cause)) => {
                let error = formatted(format_args!("{cause}"), session)?;
                return Ok(Err(Failure::Glob { glob: None, error }));
            }
        };
        let root = builder.root.copy(session)?;
        let globs = copy(originals, session)?;
        Ok(Ok(Self {
            set,
            root,
            globs,
            num_ignores,
            num_whitelists,
            // The source zero-capture Vec factory needs no retained
            // capture allocation. Only its first reached acquisition creates
            // the scratch Buffer, independently of all inner matcher caches.
            matches: None,
        }))
    }
}

fn count<A: Admission>(
    globs: &[rule::Prepared],
    whitelist: bool,
    session: &mut Session<'_, A>,
) -> Step<u64> {
    session.check()?;
    let mut count = 0usize;
    for glob in globs {
        session.items(1)?;
        if glob.whitelist == whitelist {
            count = match count.checked_add(1) {
                Some(count) => count,
                None => return session.fail(Error::Overflow),
            };
        }
    }
    match u64::try_from(count) {
        Ok(count) => Ok(count),
        Err(_) => session.fail(Error::Overflow),
    }
}

fn copy<A: Admission>(
    globs: &[rule::Prepared],
    session: &mut Session<'_, A>,
) -> Step<Vec<rule::Prepared>> {
    session.check()?;
    let layout = match Layout::array::<rule::Prepared>(globs.len()) {
        Ok(layout) => layout,
        Err(_) => return session.fail(Error::Overflow),
    };
    if !globs.is_empty() {
        session.admit(Request::Allocation {
            bytes: layout.size(),
            alignment: layout.align(),
        })?;
    }
    let mut owned = Vec::new();
    if let Err(error) = owned.try_reserve_exact(globs.len()) {
        return session.fail(Error::Allocation(error));
    }
    for glob in globs {
        session.items(1)?;
        // The source Vec clone reserves exactly len before cloning elements.
        // No subsequent capacity growth is possible in this construction.
        owned.push(glob.copy(session)?);
    }
    Ok(owned)
}

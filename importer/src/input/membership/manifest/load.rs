//! Pinned post-deserialization steps. These declarations confer no member authority.

pub(super) mod config;
pub(super) mod convert;
mod discovery;
mod driver;
pub(super) mod embedded;
pub(super) mod features;
mod ignore;
mod lints;
mod normalize;
mod path;
mod read;
mod reports;
mod roots;
mod selection;
mod workspace;

pub(super) use convert::profiles::Context as ProfileContext;
pub(super) use normalize::target::formatted;

use rawr_toml::document;

use super::{schema, session, storage, Error, Step};

/// Inputs supplied by the eventual closed loader; no ambient configuration read.
pub(super) struct Context<'a> {
    pub(super) profiles: convert::profiles::Context<'a>,
    pub(super) features: features::Context<'a>,
    pub(super) cargo_lints: bool,
}

/// Initial checks only. Warnings stay owned until complete loading succeeds.
pub(super) struct Configured<'a, 'd> {
    profiles: convert::profiles::Context<'a>,
    lint_context: lints::Context,
    features: features::Features<'a, 'd>,
    workspace: workspace::Configuration<'a, 'd>,
    warnings: Warnings,
}

/// The normalization entry retains the exact original and checked context.
/// A root declaration is published before normalization, as in pinned Cargo.
/// This is deliberately not a successfully loaded manifest.
struct Preparing<'a, 'd> {
    original: &'a schema::manifest::Original<'d>,
    path: &'a std::path::Path,
    embedded: bool,
    configured: Configured<'a, 'd>,
}

fn prepare<'a, 'd, A: document::Admission>(
    original: &'a schema::manifest::Original<'d>,
    path: &'a std::path::Path,
    embedded: bool,
    context: Context<'a>,
    roots: &mut roots::Roots<'a, 'd>,
    session: &mut session::Session<'_, A>,
) -> Step<Preparing<'a, 'd>> {
    let configured = configure(original, path, embedded, context, session)?;
    if let workspace::Configuration::Root {
        directory,
        declaration,
    } = configured.workspace
    {
        roots.insert(
            directory,
            roots::Root {
                directory,
                declaration,
            },
            session,
        )?;
    }
    session.items(1)?;
    Ok(Preparing {
        original,
        path,
        embedded,
        configured,
    })
}

pub(super) fn configure<'a, 'd, A: document::Admission>(
    original: &'a schema::manifest::Original<'d>,
    manifest_file: &'a std::path::Path,
    embedded: bool,
    context: Context<'a>,
    session: &mut session::Session<'_, A>,
) -> Step<Configured<'a, 'd>> {
    session.check()?;
    let mut warnings = Warnings::new();
    let lint_context = lints::Context {
        cargo_lints: context.cargo_lints,
        nightly_features_allowed: context.features.nightly_features_allowed,
    };
    let features =
        features::Features::interpret(original, context.features, &mut warnings, session)?;
    let workspace = workspace::configure(
        original,
        manifest_file,
        embedded,
        lint_context,
        &mut warnings,
        session,
    )?;
    session.items(1)?;
    Ok(Configured {
        profiles: context.profiles,
        lint_context,
        features,
        workspace,
        warnings,
    })
}

/// Complete warning text in source encounter order, under the caller's account.
pub(super) struct Warnings(storage::Buffer<String>);

impl Warnings {
    pub(super) const fn new() -> Self {
        Self(storage::Buffer::new())
    }

    pub(super) fn as_slice(&self) -> &[String] {
        self.0.as_slice()
    }

    /// Move a string already formatted under this Session into the report list.
    fn push_formatted<A: document::Admission>(
        &mut self,
        text: String,
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        session.check()?;
        self.0.push(text, session)?;
        Ok(())
    }

    fn push<A: document::Admission>(
        &mut self,
        parts: &[&str],
        session: &mut session::Session<'_, A>,
    ) -> Step<()> {
        session.check()?;
        let Some(length) = parts
            .iter()
            .try_fold(0usize, |sum, part| sum.checked_add(part.len()))
        else {
            return session.fail(Error::Overflow);
        };
        session.string(length)?;
        let layout = match std::alloc::Layout::array::<u8>(length) {
            Ok(layout) => layout,
            Err(_) => return session.fail(Error::Overflow),
        };
        session.admit(document::Request::Allocation {
            bytes: layout.size(),
            alignment: layout.align(),
        })?;
        let mut text = String::new();
        if let Err(error) = text.try_reserve_exact(length) {
            return session.fail(Error::Allocation(error));
        }
        #[cfg(test)]
        storage::ALLOCATIONS.with(|count| count.set(count.get() + 1));
        for part in parts {
            text.push_str(part);
        }
        self.0.push(text, session)?;
        Ok(())
    }
}

#[cfg(test)]
mod reference;
#[cfg(test)]
mod tests;

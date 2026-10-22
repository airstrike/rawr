//! Reached configuration state, borrowing stable file/document owners.

use super::super::{
    super::{session::Session, Error, Step, Stop},
    path,
};
use super::{
    cli::{self, Context},
    files::Files,
    key::Key,
    lookup::{self, Relative},
    merge::tree::{lookup::Found, Tree},
    traversal,
};
use crate::{
    cargo,
    input::membership::{host, observed::discovery::Filename},
};
use std::path::Path;

pub(in super::super) struct Owner<'a, 'image> {
    files: Files<'a, 'image>,
    context: Context<'a>,
    ordinary: Option<Tree<'a, 'a>>,
    failure: Option<Stop>,
    credentials: Option<Tree<'a, 'a>>,
    crates_io: Option<super::super::convert::source::Id<'a>>,
}

impl<'a, 'image: 'a> Owner<'a, 'image> {
    pub(super) fn from_files(files: Files<'a, 'image>, context: Context<'a>) -> Self {
        Self {
            files,
            context,
            ordinary: None,
            credentials: None,
            crates_io: None,
            failure: None,
        }
    }

    fn check(&self, session: &Session<'_, impl rawr_toml::document::Admission>) -> Step<()> {
        session.check()?;
        match self.failure {
            Some(stop) => Err(stop),
            None => Ok(()),
        }
    }

    /// Preserve a terminal owner result without changing Session's trial-parser
    /// rejection semantics. Consumed partial state cannot be queried again.
    fn retain<T>(&mut self, result: Step<T>) -> Step<T> {
        if let Err(stop) = result.as_ref() {
            self.failure.get_or_insert(*stop);
        }
        result
    }

    /// This is the reached nonempty configure/merge_cli_args stage, not all of
    /// GlobalContext::configure (which has additional getters and diagnostics).
    pub(super) fn configure_files(
        &mut self,
        arguments: &[&str],
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<()> {
        self.check(session)?;
        if arguments.is_empty() {
            return Ok(());
        }
        let normal = self.ordinary.take();
        let result = cli::load_with_files(
            &mut self.files,
            arguments,
            normal,
            self.context,
            limits,
            session,
        );
        let configured = self.retain(result)?;
        self.ordinary = Some(configured);
        Ok(())
    }

    pub(super) fn values(
        &mut self,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<&Tree<'a, 'a>> {
        self.check(session)?;
        if self.ordinary.is_none() {
            let result = traversal::load_with_files(
                &mut self.files,
                self.context.pwd,
                self.context.home,
                self.context.search_stop,
                self.context.cwd,
                limits,
                session,
            );
            let value = self.retain(result)?;
            self.ordinary = Some(value);
        }
        match self.ordinary.as_ref() {
            Some(value) => Ok(value),
            None => session.fail(Error::State),
        }
    }

    pub(super) fn load_credentials(
        &mut self,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<()> {
        self.check(session)?;
        let result = self.credentials_inner(limits, session);
        self.retain(result)
    }

    fn credentials_inner(
        &mut self,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<()> {
        if self.credentials.is_some() {
            return Ok(());
        }
        let home = path::join(Path::new(""), self.context.home, session)?;
        self.files.reset_seen();
        let Some(value) = self.files.load_discovered(
            &home,
            Filename::Credentials,
            true,
            true,
            self.context.cwd,
            limits,
            session,
        )?
        else {
            // Cargo does not fill credential_values when discovery finds none.
            return Ok(());
        };
        let value = value.normalize_credentials(session)?;
        let ordinary = self.values(limits, session)?;
        let value = value.merge_credentials(ordinary, session)?;
        self.credentials = Some(value);
        Ok(())
    }

    fn cv(
        &mut self,
        key: &Key<'_>,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Option<Found<'a>>> {
        self.check(session)?;
        if let Some(credentials) = self.credentials.as_ref()
            && let Some(value) = credentials.find(key, session)?
        {
            return Ok(Some(value));
        }
        self.values(limits, session)?.find(key, session)
    }

    /// Reached feature-gated lookup through this owner's effective configuration.
    pub(in super::super) fn path_base(
        &mut self,
        base: &super::super::super::schema::primitive::Name<'_>,
        features: &super::super::features::Features<'_, '_>,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Option<std::path::PathBuf>> {
        self.check(session)?;
        let key = Key::declared_path_base(base, features, session)?;
        let cwd = self.context.cwd;
        match self.optional_path(&key, limits, session)? {
            Some(relative) => relative.resolve(cwd, session).map(Some),
            None => Ok(None),
        }
    }

    pub(super) fn optional_path<'r>(
        &mut self,
        key: &'r Key<'_>,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Option<Relative<'r>>>
    where
        'a: 'r,
    {
        self.check(session)?;
        let result =
            lookup::optional_path(key, session, |key, session| self.cv(key, limits, session));
        self.retain(result)
    }

    pub(super) fn optional_string<'r>(
        &mut self,
        key: &'r Key<'_>,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Option<lookup::string::Value<'r>>>
    where
        'a: 'r,
    {
        self.check(session)?;
        let result =
            lookup::string::get(key, session, |key, session| self.cv(key, limits, session));
        self.retain(result)
    }

    pub(in super::super) fn crates_io(
        &mut self,
        sources: &mut super::super::convert::source::Owner<'a>,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Result<super::super::convert::source::Id<'a>, String>> {
        self.check(session)?;
        let result = (|| {
            if let Some(id) = self.crates_io {
                sources.get(id, session)?;
                return Ok(Ok(id));
            }
            if let Err(error) = self.check_registry_index_not_set(limits, session)? {
                return Ok(Err(error));
            }
            let url = match super::super::convert::url::parse(
                "https://github.com/rust-lang/crates.io-index",
                session,
            )? {
                Ok(url) => url,
                Err(error) => return Ok(Err(error)),
            };
            let id = match sources.registry(&url, Some("crates-io"), session)? {
                Ok(id) => id,
                Err(error) => return Ok(Err(error)),
            };
            self.crates_io = Some(id);
            Ok(Ok(id))
        })();
        self.retain(result)
    }

    /// Resolve a validated registry name through its defining configuration.
    pub(in super::super) fn registry_index(
        &mut self,
        registry: super::super::super::scalar::Name<'_>,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Result<super::super::super::spec::url::Url, String>> {
        use super::super::super::spec::url::BaseError;
        use super::super::{convert::url, formatted};
        self.check(session)?;
        let name = registry.as_str();
        let key = Key::registry(registry, session)?;
        let Some(index) = self.optional_string(&key, limits, session)? else {
            return formatted(
                format_args!("registry index was not found in any configuration: `{name}`"),
                session,
            )
            .map(Err);
        };
        let definition = index.definition();
        let root = definition.root(self.context.cwd, session)?;
        let base = path::join(root, Path::new("truncated-by-url_with_base"), session)?;
        let text = index.text();
        let resolved = match url::accepted(text, session)? {
            Err(error) => Err(error),
            Ok(accepted) => match accepted.with_file_base(&base, session)? {
                Err(BaseError::Path) => {
                    let cause = formatted(
                        format_args!("invalid path url `{}`", base.display()),
                        session,
                    )?;
                    Err(formatted(
                        format_args!("invalid url `{text}`: {cause}"),
                        session,
                    )?)
                }
                Err(BaseError::Url(error)) => Err(formatted(
                    format_args!("invalid url `{text}`: {error}"),
                    session,
                )?),
                Ok(url) => {
                    if url.password(session)?.is_some() {
                        Err(formatted(
                            format_args!("registry URLs may not contain passwords"),
                            session,
                        )?)
                    } else {
                        Ok(url)
                    }
                }
            },
        };
        match resolved {
            Ok(url) => Ok(Ok(url)),
            Err(cause) => {
                let context = formatted(
                    format_args!("invalid index URL for registry `{name}` defined in {definition}"),
                    session,
                )?;
                formatted(format_args!("{context}: {cause}"), session).map(Err)
            }
        }
    }

    /// Cargo checks this obsolete key before constructing the crates.io source.
    pub(in super::super) fn check_registry_index_not_set(
        &mut self,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Result<(), String>> {
        self.check(session)?;
        let key = Key::registry_index(session)?;
        if self.optional_string(&key, limits, session)?.is_some() {
            return super::super::formatted(format_args!("the `registry.index` config value is no longer supported\nUse `[source]` replacement to alter the default index for crates.io."), session).map(Err);
        }
        Ok(Ok(()))
    }
}

#[cfg(test)]
mod tests;

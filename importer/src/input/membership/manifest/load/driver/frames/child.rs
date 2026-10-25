//! Associate a child read and its finalized return with one retained parent.
use super::*;
use crate::input::membership::manifest::load::{read, Context};
use std::os::unix::ffi::OsStrExt;

pub(in crate::input::membership::manifest::load) use read::{Cause, Stage};
pub(in crate::input::membership::manifest::load) struct Failed<'a, 'd, 'owner, 's> {
    pub(in crate::input::membership::manifest::load) parent: Waiting<'a, 'd, 'owner, 's>,
    pub(in crate::input::membership::manifest::load) stage: Stage,
}
pub(in crate::input::membership::manifest::load) struct BeginFailure<'a, 'd, 'owner, 's> {
    pub(in crate::input::membership::manifest::load) frame: Failed<'a, 'd, 'owner, 's>,
    pub(in crate::input::membership::manifest::load) cause: Cause,
}

pub(in crate::input::membership::manifest::load) struct Awaiting<'a, 'd, 'owner, 's> {
    parent: Waiting<'a, 'd, 'owner, 's>,
    child: source::Id<'s>,
}
impl<'a, 'd: 'a, 'owner, 's> Waiting<'a, 'd, 'owner, 's> {
    #[allow(clippy::too_many_arguments)]
    pub(in crate::input::membership::manifest::load) fn begin<'body: 'd, 'image: 'body>(
        self,
        reader: &mut read::Reader<'body, 'd, 'a, 'image>,
        script: bool,
        context: Context<'a>,
        sources: &mut source::Owner<'s>,
        roots: &mut roots::Roots<'a, 'd>,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<
        Result<
            (Awaiting<'a, 'd, 'owner, 's>, read::Sourced<'a, 'd, 's>),
            BeginFailure<'a, 'd, 'owner, 's>,
        >,
    > {
        let child = match reader
            .begin_staged(self.path, script, context, sources, roots, limits, session)?
        {
            Ok(child) => child,
            Err(failure) => {
                return Ok(Err(BeginFailure {
                    frame: Failed {
                        parent: self,
                        stage: failure.stage,
                    },
                    cause: failure.cause,
                }))
            }
        };
        Ok(Ok((
            Awaiting {
                child: child.source,
                parent: self,
            },
            child,
        )))
    }
}
impl<'a, 'd, 'owner, 's> Awaiting<'a, 'd, 'owner, 's> {
    pub(in crate::input::membership::manifest::load) fn parent(
        &self,
    ) -> &Waiting<'a, 'd, 'owner, 's> {
        &self.parent
    }
    pub(in crate::input::membership::manifest::load) fn complete(
        self,
        child: super::super::convert::Finished<'a, 'd, '_, '_, 's>,
        roots: &mut roots::Roots<'a, 'd>,
        session: &mut Session<'_, host::Host<'_, '_>>,
    ) -> Step<Result<Frame<'a, 'd, 'owner, 's>, (Waiting<'a, 'd, 'owner, 's>, String)>> {
        session.check()?;
        let expected = self.parent.path.as_os_str().as_bytes();
        let actual = child.owner.manifest().as_os_str().as_bytes();
        session.items(expected.len())?;
        session.items(actual.len())?;
        if self.child != child.source || expected != actual {
            return session.fail(Error::State);
        }
        let Waiting {
            source,
            progress,
            candidates,
            path,
        } = self.parent;
        let search = match candidates {
            None => match child.inheritable(roots, session)? {
                Ok(root) => Search::Selected(super::super::super::discovery::Lookup::Cached(root)),
                Err(error) => {
                    return Ok(Err((
                        Waiting {
                            source,
                            progress,
                            path,
                            candidates: None,
                        },
                        error,
                    )))
                }
            },
            Some(candidates) => {
                let Some(request) = progress.workspace_request() else {
                    return session.fail(Error::State);
                };
                match child.owner.workspace().selected_root(
                    child.owner.manifest(),
                    request.manifest,
                    session,
                )? {
                    Some(path) => Search::Selected(super::super::super::discovery::lookup(
                        path, roots, session,
                    )?),
                    None => Search::Candidates(candidates),
                }
            }
        };
        Ok(Ok(Frame {
            source,
            progress,
            search,
        }))
    }
}

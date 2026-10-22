//! Flat, admitted parent continuations; no recursive loader callback.
use super::*;
use crate::input::membership::manifest::{load::convert::source, storage};
mod child;
pub(in super::super) use child::{Cause, Stage};

pub(in super::super) struct Frame<'a, 'd, 'owner, 's> {
    pub(in super::super) source: source::Id<'s>,
    pub(in super::super) progress: Progress<'a, 'd, 'owner>,
    pub(in super::super) search: Search<'a, 'a, 'd>,
}
pub(in super::super) enum Action<'a, 'd, 'owner, 's> {
    Root {
        source: source::Id<'s>,
        progress: Progress<'a, 'd, 'owner>,
        root: roots::Root<'a, 'd>,
    },
    Load {
        source: source::Id<'s>,
        progress: Progress<'a, 'd, 'owner>,
        path: std::path::PathBuf,
        /// None means inheritable_from_path; Some retains discovery after a
        /// child whose completed configuration may decline to select a root.
        candidates: Option<super::super::discovery::Candidates<'a>>,
    },
}
/// The child's pathname lives in an external tier, independently of this
/// movable parent continuation. No source read or child success is implied.
pub(in super::super) struct Waiting<'a, 'd, 'owner, 's> {
    pub(in super::super) source: source::Id<'s>,
    pub(in super::super) progress: Progress<'a, 'd, 'owner>,
    pub(in super::super) path: &'a std::path::Path,
    pub(in super::super) candidates: Option<super::super::discovery::Candidates<'a>>,
}
impl<'a, 'd, 'owner, 's> Action<'a, 'd, 'owner, 's> {
    /// A cached root returns unchanged and requires no pathname slot.
    pub(in super::super) fn retain_path<A: rawr_toml::document::Admission>(
        self,
        paths: &mut slots::Writer<'a, std::path::PathBuf>,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Waiting<'a, 'd, 'owner, 's>, Self>> {
        session.check()?;
        match self {
            Self::Load {
                source,
                progress,
                path,
                candidates,
            } => {
                let path = paths.push(path, session)?.as_path();
                Ok(Ok(Waiting {
                    source,
                    progress,
                    path,
                    candidates,
                }))
            }
            action @ Self::Root { .. } => Ok(Err(action)),
        }
    }
}
impl<'a, 'd, 'owner, 's> Frame<'a, 'd, 'owner, 's> {
    pub(in super::super) fn advance(
        self,
        session: &mut Session<'_, host::Host<'_, '_>>,
    ) -> Step<Result<Action<'a, 'd, 'owner, 's>, (Self, String)>> {
        session.check()?;
        let Self {
            source,
            progress,
            search,
        } = self;
        let (path, candidates) = match search {
            Search::Selected(super::super::discovery::Lookup::Cached(root)) => {
                return Ok(Ok(Action::Root {
                    source,
                    progress,
                    root,
                }));
            }
            Search::Selected(super::super::discovery::Lookup::Load(path)) => (path, None),
            Search::Candidates(mut candidates) => {
                let Some(path) = candidates.next(session)? else {
                    let error = super::super::formatted(
                        format_args!("failed to find a workspace root"),
                        session,
                    )?;
                    return Ok(Err((
                        Self {
                            source,
                            progress,
                            search: Search::Candidates(candidates),
                        },
                        error,
                    )));
                };
                (path, Some(candidates))
            }
        };
        Ok(Ok(Action::Load {
            source,
            progress,
            path,
            candidates,
        }))
    }
}
pub(in super::super) enum Continuation<'a, 'd, 'owner, 's> {
    Search(Frame<'a, 'd, 'owner, 's>),
    Child(child::Awaiting<'a, 'd, 'owner, 's>),
    Failed(child::Failed<'a, 'd, 'owner, 's>),
    ManifestFailed(&'a std::path::Path),
    SearchFailed(Frame<'a, 'd, 'owner, 's>),
    ReadFailed(&'a std::path::Path, Stage),
}
pub(in super::super) struct Stack<'a, 'd, 'owner, 's>(
    storage::Buffer<Continuation<'a, 'd, 'owner, 's>>,
);
impl<'a, 'd, 'owner, 's> Stack<'a, 'd, 'owner, 's> {
    pub(in super::super) const fn new() -> Self {
        Self(storage::Buffer::new())
    }
    pub(in super::super) fn suspend<'image: 'a>(
        &mut self,
        progress: Progress<'a, 'd, 'owner>,
        source: source::Id<'s>,
        home: &'a std::path::Path,
        roots: &roots::Roots<'a, 'd>,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<()> {
        session.check()?;
        let Some(request) = progress.workspace_request() else {
            return session.fail(Error::State);
        };
        let search = request.search(home, roots, session)?;
        self.0.push(
            Continuation::Search(Frame {
                source,
                progress,
                search,
            }),
            session,
        )?;
        Ok(())
    }
    /// Save the caller before normalizing a child that may itself suspend.
    pub(in super::super) fn await_child<A: rawr_toml::document::Admission>(
        &mut self,
        parent: child::Awaiting<'a, 'd, 'owner, 's>,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        self.0.push(Continuation::Child(parent), session)?;
        Ok(())
    }
    pub(in super::super) fn pop<A: rawr_toml::document::Admission>(
        &mut self,
        session: &mut Session<'_, A>,
    ) -> Step<Option<Continuation<'a, 'd, 'owner, 's>>> {
        session.check()?;
        Ok(self.0.pop())
    }
    pub(in super::super) fn failed<A: rawr_toml::document::Admission>(
        &mut self,
        failure: child::Failed<'a, 'd, 'owner, 's>,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        self.0.push(Continuation::Failed(failure), session)?;
        Ok(())
    }
    pub(in super::super) fn failed_manifest<A: rawr_toml::document::Admission>(
        &mut self,
        path: &'a std::path::Path,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        self.0.push(Continuation::ManifestFailed(path), session)?;
        Ok(())
    }
    pub(in super::super) fn has_failure<A: rawr_toml::document::Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        session.check()?;
        Ok(matches!(
            self.0.as_slice().last(),
            Some(
                Continuation::Failed(_)
                    | Continuation::ManifestFailed(_)
                    | Continuation::SearchFailed(_)
                    | Continuation::ReadFailed(_, _)
            )
        ))
    }
    pub(in super::super) fn failed_search<A: rawr_toml::document::Admission>(
        &mut self,
        frame: Frame<'a, 'd, 'owner, 's>,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        self.0.push(Continuation::SearchFailed(frame), session)?;
        Ok(())
    }
    pub(in super::super) fn failed_read<A: rawr_toml::document::Admission>(
        &mut self,
        path: &'a std::path::Path,
        stage: Stage,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        self.0
            .push(Continuation::ReadFailed(path, stage), session)?;
        Ok(())
    }
}

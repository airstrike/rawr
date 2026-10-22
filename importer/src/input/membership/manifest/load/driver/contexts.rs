//! Cargo's child-load context chain, assembled inner to outer without recursion.
use super::*;
use crate::input::membership::manifest::storage::Buffer;
use rawr_toml::document::Admission;

impl Progress<'_, '_, '_> {
    fn failure_context<A: Admission>(&self, session: &mut Session<'_, A>) -> Step<String> {
        match self {
            Self::Patches { pending, .. } => pending.failure_context(session),
            Self::Package { pending, .. } => pending.failure_context(session),
            Self::Tail {
                progress: Some(tail::Progress::Workspace(pending)),
                ..
            } => pending.failure_context(session),
            _ => session.fail(Error::State),
        }
    }
}

impl frames::Waiting<'_, '_, '_, '_> {
    fn append_contexts<A: Admission>(
        &self,
        contexts: &mut Buffer<String>,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        let Some(request) = self.progress.workspace_request() else {
            return session.fail(Error::State);
        };
        if self.candidates.is_some() {
            let message = super::super::formatted(format_args!(
                "failed searching for potential workspace\npackage manifest: `{}`\ninvalid potential workspace manifest: `{}`\n\nhelp: to avoid searching for a non-existent workspace, add `[workspace]` to the package manifest",
                request.manifest.display(), self.path.display()), session)?;
            contexts.push(message, session)?;
        }
        contexts.push(self.progress.failure_context(session)?, session)?;
        manifest(request.manifest, contexts, session)
    }
}

fn manifest<A: Admission>(
    path: &std::path::Path,
    contexts: &mut Buffer<String>,
    session: &mut Session<'_, A>,
) -> Step<()> {
    let message = super::super::formatted(
        format_args!("failed to parse manifest at `{}`", path.display()),
        session,
    )?;
    contexts.push(message, session)?;
    Ok(())
}

impl Stack<'_, '_, '_, '_> {
    pub(in super::super) fn caller_contexts<A: Admission>(
        &mut self,
        caller: &frames::Waiting<'_, '_, '_, '_>,
        session: &mut Session<'_, A>,
    ) -> Step<Buffer<String>> {
        session.check()?;
        let mut contexts = Buffer::new();
        caller.append_contexts(&mut contexts, session)?;
        self.append_parents(&mut contexts, session)?;
        Ok(contexts)
    }
    /// Consume only a retained semantic begin failure. The caller retains its
    /// original leaf. A latched refusal exits before allocation or stack work.
    pub(in super::super) fn rejected_contexts<A: Admission>(
        &mut self,
        session: &mut Session<'_, A>,
    ) -> Step<Buffer<String>> {
        session.check()?;
        let mut contexts = Buffer::new();
        match self.pop(session)? {
            Some(Continuation::ReadFailed(path, stage)) => {
                if matches!(stage, frames::Stage::Configuration) {
                    manifest(path, &mut contexts, session)?;
                }
            }
            Some(Continuation::Failed(failed)) => {
                if matches!(failed.stage, frames::Stage::Configuration) {
                    manifest(failed.parent.path, &mut contexts, session)?;
                }
                failed.parent.append_contexts(&mut contexts, session)?;
            }
            Some(Continuation::ManifestFailed(path)) => manifest(path, &mut contexts, session)?,
            Some(Continuation::SearchFailed(frame)) => {
                let Some(request) = frame.progress.workspace_request() else {
                    return session.fail(Error::State);
                };
                contexts.push(frame.progress.failure_context(session)?, session)?;
                manifest(request.manifest, &mut contexts, session)?;
            }
            _ => return session.fail(Error::State),
        }
        self.append_parents(&mut contexts, session)?;
        Ok(contexts)
    }
    pub(in super::super) fn manifest_contexts<A: Admission>(
        &mut self,
        path: &std::path::Path,
        session: &mut Session<'_, A>,
    ) -> Step<Buffer<String>> {
        session.check()?;
        let mut contexts = Buffer::new();
        manifest(path, &mut contexts, session)?;
        self.append_parents(&mut contexts, session)?;
        Ok(contexts)
    }
    fn append_parents<A: Admission>(
        &mut self,
        contexts: &mut Buffer<String>,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        while let Some(continuation) = self.pop(session)? {
            let Continuation::Child(child) = continuation else {
                return session.fail(Error::State);
            };
            child.parent().append_contexts(contexts, session)?;
        }
        Ok(())
    }
}

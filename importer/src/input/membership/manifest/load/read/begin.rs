//! Shared source-ordered entry for top-level and nested manifest reads.
use super::*;
use crate::input::membership::manifest::{Diagnostic, Stop};

pub(in crate::input::membership::manifest::load) enum Stage {
    Source,
    Interpretation,
    Configuration,
}
pub(in crate::input::membership::manifest::load) enum Cause {
    Message(String),
    Rejected(Diagnostic),
}
pub(in crate::input::membership::manifest::load) struct Failure {
    pub(in crate::input::membership::manifest::load) stage: Stage,
    pub(in crate::input::membership::manifest::load) cause: Cause,
}
impl<'body, 'document, 'original, 'image> Reader<'body, 'document, 'original, 'image>
where
    'image: 'body,
    'body: 'document,
    'document: 'original,
{
    pub(in crate::input::membership::manifest::load) fn begin_staged<'s>(
        &mut self,
        path: &'original Path,
        script: bool,
        mut context: Context<'original>,
        sources: &mut source::Owner<'s>,
        roots: &mut Roots<'original, 'document>,
        limits: &cargo::Limits,
        session: &mut Session<'_, host::Host<'image, '_>>,
    ) -> Step<Result<Sourced<'original, 'document, 's>, Failure>> {
        let fail = |stage, cause| Ok(Err(Failure { stage, cause }));
        let source = match sources.manifest(path, session) {
            Ok(Ok(source)) => source,
            Ok(Err(message)) => return fail(Stage::Source, Cause::Message(message)),
            Err(Stop::Rejected(diagnostic)) => {
                return fail(Stage::Source, Cause::Rejected(diagnostic))
            }
            Err(Stop::Refused) => return Err(Stop::Refused),
        };
        // for_manifest_path constructs a Path source, independent of input flags.
        context.features.is_local = true;
        let (original, embedded) = match self.interpret(path, script, limits, session) {
            Ok(original) => original,
            Err(Stop::Rejected(diagnostic)) => {
                return fail(Stage::Interpretation, Cause::Rejected(diagnostic))
            }
            Err(Stop::Refused) => return Err(Stop::Refused),
        };
        let preparing =
            match super::super::prepare(original, path, embedded, context, roots, session) {
                Ok(preparing) => preparing,
                Err(Stop::Rejected(diagnostic)) => {
                    return fail(Stage::Configuration, Cause::Rejected(diagnostic))
                }
                Err(Stop::Refused) => return Err(Stop::Refused),
            };
        Ok(Ok(Sourced { source, preparing }))
    }
}

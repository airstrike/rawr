//! Iterative successful loading through conversion and finalized child return.
//! Semantic conversion failures unwind context; native work retains the stack.
use super::*;
use crate::input::membership::manifest::{
    load::{
        convert::{package, source},
        read, Context,
    },
    spec::Spec,
    Stop,
};

pub(in super::super) enum Outcome<'a, 'd, 'owner, 'tail, 's> {
    Complete(convert::Finished<'a, 'd, 'owner, 'tail, 's>),
    Conversion(Report<'a, 'd, 'owner, 'tail, 's>),
    Failed {
        report: Report<'a, 'd, 'owner, 'tail, 's>,
        contexts: crate::input::membership::manifest::storage::Buffer<String>,
    },
    Rejected {
        cause: frames::Cause,
        contexts: crate::input::membership::manifest::storage::Buffer<String>,
    },
}

pub(in super::super) fn run<
    'body: 'd,
    'd: 'a,
    'a: 'owner,
    'owner: 'tail,
    'tail,
    's: 'tail,
    'image: 'body + 's,
>(
    mut work: Work<'a, 'd, 'owner, 's>,
    stack: &mut Stack<'a, 'd, 'owner, 's>,
    reader: &mut read::Reader<'body, 'd, 'a, 'image>,
    paths: &mut slots::Writer<'a, std::path::PathBuf>,
    targets: &mut slots::Writer<'owner, all_targets::Owner<'a, 'd>>,
    tails: &mut slots::Writer<'tail, tail::Complete<'owner, 'd>>,
    specs: &mut slots::Writer<'tail, Spec>,
    packages: &mut package::Owner<'tail, 's>,
    sources: &mut source::Owner<'s>,
    config: &mut config::Owner<'s, 'image>,
    roots: &mut roots::Roots<'a, 'd>,
    home: &'a std::path::Path,
    context: &Context<'a>,
    script: bool,
    public_dependency: bool,
    bindeps: bool,
    json_target_spec: bool,
    limits: &cargo::Limits,
    session: &mut Session<'_, host::Host<'image, '_>>,
) -> Step<Result<Outcome<'a, 'd, 'owner, 'tail, 's>, String>> {
    loop {
        let active = match walk(
            work,
            stack,
            reader,
            paths,
            targets,
            sources,
            config,
            roots,
            home,
            context,
            script,
            public_dependency,
            limits,
            session,
        ) {
            Ok(Ok(active)) => active,
            Ok(Err(error)) => {
                if stack.has_failure(session)? {
                    let contexts = stack.rejected_contexts(session)?;
                    return Ok(Ok(Outcome::Rejected {
                        cause: frames::Cause::Message(error),
                        contexts,
                    }));
                }
                return Ok(Err(error));
            }
            Err(Stop::Rejected(diagnostic)) => {
                if stack.has_failure(session)? {
                    let contexts = stack.rejected_contexts(session)?;
                    return Ok(Ok(Outcome::Rejected {
                        cause: frames::Cause::Rejected(diagnostic),
                        contexts,
                    }));
                }
                return Err(Stop::Rejected(diagnostic));
            }
            Err(Stop::Refused) => return Err(Stop::Refused),
        };
        let report = active.progress.convert(
            tails,
            specs,
            packages,
            active.source,
            sources,
            config,
            bindeps,
            json_target_spec,
            limits,
            session,
        )?;
        let finished = match report.finish(session)? {
            Ok(finished) => finished,
            Err(report) => {
                if report.result.is_err() {
                    let contexts = stack.manifest_contexts(report.owner.manifest(), session)?;
                    return Ok(Ok(Outcome::Failed { report, contexts }));
                }
                return Ok(Ok(Outcome::Conversion(report)));
            }
        };
        match stack.pop(session)? {
            None => return Ok(Ok(Outcome::Complete(finished))),
            Some(Continuation::Child(parent)) => {
                work = match parent.complete(finished, roots, session)? {
                    Ok(frame) => Work::Search(frame),
                    Err((caller, error)) => {
                        let contexts = stack.caller_contexts(&caller, session)?;
                        return Ok(Ok(Outcome::Rejected {
                            cause: frames::Cause::Message(error),
                            contexts,
                        }));
                    }
                };
            }
            Some(
                Continuation::Search(_)
                | Continuation::Failed(_)
                | Continuation::ManifestFailed(_)
                | Continuation::SearchFailed(_)
                | Continuation::ReadFailed(_, _),
            ) => return session.fail(Error::State),
        }
    }
}

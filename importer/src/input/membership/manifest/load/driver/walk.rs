//! Iterative normalization/discovery descent. Conversion remains an explicit
//! boundary so native suspensions retain the same stack and caller state.
use super::*;
use crate::input::membership::manifest::load::{convert::source, read, Context};
use crate::input::membership::manifest::Stop;

pub(in super::super) enum Work<'a, 'd, 'owner, 's> {
    Read(&'a std::path::Path),
    Normalize(read::Normalizing<'a, 'd, 'owner, 's>),
    Search(frames::Frame<'a, 'd, 'owner, 's>),
}
#[allow(clippy::too_many_arguments)]
pub(in super::super) fn walk<'body: 'd, 'd: 'a, 'a: 'owner, 'owner, 's, 'image: 'body + 's>(
    mut work: Work<'a, 'd, 'owner, 's>,
    stack: &mut Stack<'a, 'd, 'owner, 's>,
    reader: &mut read::Reader<'body, 'd, 'a, 'image>,
    paths: &mut slots::Writer<'a, std::path::PathBuf>,
    targets: &mut slots::Writer<'owner, all_targets::Owner<'a, 'd>>,
    sources: &mut source::Owner<'s>,
    config: &mut config::Owner<'s, 'image>,
    roots: &mut roots::Roots<'a, 'd>,
    home: &'a std::path::Path,
    context: &Context<'a>,
    script: bool,
    public_dependency: bool,
    limits: &cargo::Limits,
    session: &mut Session<'_, host::Host<'image, '_>>,
) -> Step<Result<read::Normalizing<'a, 'd, 'owner, 's>, String>> {
    loop {
        session.check()?;
        let frame = match work {
            Work::Read(path) => {
                let context = Context {
                    profiles: context.profiles,
                    features: context.features,
                    cargo_lints: context.cargo_lints,
                };
                let sourced = match reader
                    .begin_staged(path, script, context, sources, roots, limits, session)?
                {
                    Ok(sourced) => sourced,
                    Err(failure) => {
                        stack.failed_read(path, failure.stage, session)?;
                        return match failure.cause {
                            frames::Cause::Message(error) => Ok(Err(error)),
                            frames::Cause::Rejected(diagnostic) => Err(Stop::Rejected(diagnostic)),
                        };
                    }
                };
                let active =
                    match sourced.normalize(targets, config, public_dependency, limits, session) {
                        Ok(Ok(active)) => active,
                        Ok(Err(error)) => {
                            stack.failed_manifest(path, session)?;
                            return Ok(Err(error));
                        }
                        Err(Stop::Rejected(diagnostic)) => {
                            stack.failed_manifest(path, session)?;
                            return Err(Stop::Rejected(diagnostic));
                        }
                        Err(Stop::Refused) => return Err(Stop::Refused),
                    };
                work = Work::Normalize(active);
                continue;
            }
            Work::Normalize(active) => {
                if active.progress.workspace_request().is_none() {
                    return Ok(Ok(active));
                }
                stack.suspend(active.progress, active.source, home, roots, session)?;
                let Some(Continuation::Search(frame)) = stack.pop(session)? else {
                    return session.fail(Error::State);
                };
                frame
            }
            Work::Search(frame) => frame,
        };
        let action = match frame.advance(session)? {
            Ok(action) => action,
            Err((frame, error)) => {
                stack.failed_search(frame, session)?;
                return Ok(Err(error));
            }
        };
        work = match action {
            Action::Root {
                source,
                progress,
                root,
            } => {
                let Some(request) = progress.workspace_request() else {
                    return session.fail(Error::State);
                };
                let manifest = request.manifest;
                let progress =
                    match progress.resume_declared(root, targets, config, limits, session) {
                        Ok(Ok(progress)) => progress,
                        Ok(Err(error)) => {
                            stack.failed_manifest(manifest, session)?;
                            return Ok(Err(error));
                        }
                        Err(Stop::Rejected(diagnostic)) => {
                            stack.failed_manifest(manifest, session)?;
                            return Err(Stop::Rejected(diagnostic));
                        }
                        Err(Stop::Refused) => return Err(Stop::Refused),
                    };
                Work::Normalize(read::Normalizing { source, progress })
            }
            action @ Action::Load { .. } => {
                let Ok(waiting) = action.retain_path(paths, session)? else {
                    return session.fail(Error::State);
                };
                let context = Context {
                    profiles: context.profiles,
                    features: context.features,
                    cargo_lints: context.cargo_lints,
                };
                let child_path = waiting.path;
                let (awaiting, child) = match waiting
                    .begin(reader, script, context, sources, roots, limits, session)?
                {
                    Ok(child) => child,
                    Err(failure) => {
                        stack.failed(failure.frame, session)?;
                        return match failure.cause {
                            frames::Cause::Message(error) => Ok(Err(error)),
                            frames::Cause::Rejected(diagnostic) => Err(
                                crate::input::membership::manifest::Stop::Rejected(diagnostic),
                            ),
                        };
                    }
                };
                stack.await_child(awaiting, session)?;
                let child =
                    match child.normalize(targets, config, public_dependency, limits, session) {
                        Ok(Ok(child)) => child,
                        Ok(Err(error)) => {
                            stack.failed_manifest(child_path, session)?;
                            return Ok(Err(error));
                        }
                        Err(Stop::Rejected(diagnostic)) => {
                            stack.failed_manifest(child_path, session)?;
                            return Err(Stop::Rejected(diagnostic));
                        }
                        Err(Stop::Refused) => return Err(Stop::Refused),
                    };
                Work::Normalize(child)
            }
        };
    }
}

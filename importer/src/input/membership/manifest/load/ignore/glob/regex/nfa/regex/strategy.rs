//! Reached strategy::new guards for the nonempty globset-emitted grammar.
use super::{info, Admission, Error, Session, Step};

pub(super) fn prefilter<A: Admission>(info: &info::Info, session: &mut Session<'_, A>) -> Step<()> {
    session.items(1)?;
    if info.anchored_start() {
        return Ok(());
    }
    // This would reach configured/automatic prefilter construction in source.
    // A different producer needs that construction, not a fabricated None.
    session.fail(Error::State)
}

pub(super) fn finish<A: Admission>(info: &info::Info, session: &mut Session<'_, A>) -> Step<()> {
    reverse_anchored(info, session)?;
    reverse_suffix(info, session)?;
    reverse_inner(info, session)
}

fn reverse_anchored<A: Admission>(info: &info::Info, session: &mut Session<'_, A>) -> Step<()> {
    session.items(1)?;
    if !info.anchored_end() {
        return Ok(());
    }
    session.items(1)?;
    if info.anchored_start() {
        return Ok(());
    }
    session.fail(Error::State)
}

fn reverse_suffix<A: Admission>(info: &info::Info, session: &mut Session<'_, A>) -> Step<()> {
    session.items(1)?;
    if !info.auto_prefilter() {
        return Ok(());
    }
    session.items(1)?;
    if info.anchored_start() {
        return Ok(());
    }
    session.fail(Error::State)
}

fn reverse_inner<A: Admission>(info: &info::Info, session: &mut Session<'_, A>) -> Step<()> {
    session.items(1)?;
    if !info.auto_prefilter() {
        return Ok(());
    }
    session.items(1)?;
    if info.kind() != info::Kind::LeftmostFirst {
        return Ok(());
    }
    session.items(1)?;
    if info.anchored_start() {
        return Ok(());
    }
    session.fail(Error::State)
}

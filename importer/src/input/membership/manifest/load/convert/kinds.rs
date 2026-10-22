//! Ordered package CompileTarget construction; JSON requests remain pending.
use super::dependency::compile;
use crate::input::membership::manifest::{
    load::normalize::{library, package},
    schema::package::Field,
    session::Session,
    Error, Step,
};
use rawr_toml::document;

pub(in super::super) struct Ready {
    pub(in super::super) default: Option<compile::Target>,
    pub(in super::super) forced: Option<compile::Target>,
}
pub(in super::super) enum Progress<'a, 'd> {
    Ready(Ready),
    Pending(Pending<'a, 'd>),
}
pub(in super::super) enum Pending<'a, 'd> {
    Default {
        request: compile::Request<'a>,
        forced: Deferred<'a, 'd>,
    },
    Forced {
        default: Option<compile::Target>,
        request: compile::Request<'a>,
    },
}
pub(in super::super) enum Deferred<'a, 'd> {
    Package(Option<&'a package::View<'a, 'd>>),
    #[cfg(test)]
    Spelling(Option<&'a str>),
}
impl<'a, 'd> Deferred<'a, 'd> {
    fn read<A: document::Admission>(self, session: &mut Session<'_, A>) -> Step<Option<&'a str>> {
        match self {
            Self::Package(package) => text(package, Field::ForcedTarget, session),
            #[cfg(test)]
            Self::Spelling(value) => Ok(value),
        }
    }
}
pub(in super::super) fn convert<'a, 'd, A: document::Admission>(
    package: Option<&'a package::View<'a, 'd>>,
    json: bool,
    session: &mut Session<'_, A>,
) -> Step<Result<Progress<'a, 'd>, String>> {
    let default = text(package, Field::DefaultTarget, session)?;
    select(default, Deferred::Package(package), json, session)
}
fn text<'a, A: document::Admission>(
    package: Option<&'a package::View<'_, '_>>,
    field: Field,
    session: &mut Session<'_, A>,
) -> Step<Option<&'a str>> {
    session.items(1)?;
    match package.and_then(|package| package.get(field)) {
        None => Ok(None),
        Some(value) => match library::text(value) {
            Some(value) => Ok(Some(value)),
            None => session.fail(Error::State),
        },
    }
}
fn select<'a, 'd, A: document::Admission>(
    default: Option<&'a str>,
    forced: Deferred<'a, 'd>,
    json: bool,
    session: &mut Session<'_, A>,
) -> Step<Result<Progress<'a, 'd>, String>> {
    session.check()?;
    let default = match default {
        None => None,
        Some(value) => match compile::compile(value, json, session)? {
            Err(error) => return Ok(Err(error)),
            Ok(compile::Progress::Ready(target)) => Some(target),
            Ok(compile::Progress::Canonical(request)) => {
                return Ok(Ok(Progress::Pending(Pending::Default { request, forced })))
            }
        },
    };
    let forced = match forced.read(session)? {
        None => None,
        Some(value) => match compile::compile(value, json, session)? {
            Err(error) => return Ok(Err(error)),
            Ok(compile::Progress::Ready(target)) => Some(target),
            Ok(compile::Progress::Canonical(request)) => {
                return Ok(Ok(Progress::Pending(Pending::Forced { default, request })))
            }
        },
    };
    Ok(Ok(Progress::Ready(Ready { default, forced })))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::reference;
    #[test]
    fn default_precedes_forced_and_pending_retains_prior_selection() {
        let mut account = reference::Account;
        let mut session = Session::new(&mut account);
        let Ok(Err(error)) = select(
            Some(" "),
            Deferred::Spelling(Some("bad.json")),
            false,
            &mut session,
        ) else {
            panic!("empty")
        };
        assert_eq!(error, "target was empty");
        let Ok(Ok(Progress::Pending(Pending::Default { request, forced }))) = select(
            Some(" x.json "),
            Deferred::Spelling(Some(" ")),
            true,
            &mut session,
        ) else {
            panic!("default pending")
        };
        assert_eq!(request.spelling(), "x.json");
        assert!(matches!(forced, Deferred::Spelling(Some(" "))));
        let Ok(Ok(Progress::Pending(Pending::Forced {
            default: Some(compile::Target::Tuple(default)),
            request,
        }))) = select(
            Some("target"),
            Deferred::Spelling(Some("x.json")),
            true,
            &mut session,
        )
        else {
            panic!("forced pending")
        };
        assert_eq!(default, "target");
        assert_eq!(request.spelling(), "x.json");
        let Ok(Ok(Progress::Ready(Ready {
            default: None,
            forced: Some(compile::Target::Tuple(forced)),
        }))) = select(
            None,
            Deferred::Spelling(Some(" triple ")),
            false,
            &mut session,
        )
        else {
            panic!("ready")
        };
        assert_eq!(forced, "triple");
    }
}

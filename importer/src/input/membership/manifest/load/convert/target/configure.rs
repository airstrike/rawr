// Copyright the Cargo contributors. MIT OR Apache-2.0.
// License texts: ../../path/LICENSE-MIT and ../../path/LICENSE-APACHE.
//! Pinned Target constructor defaults and configure overrides.

use super::super::super::super::{schema::Target, session::Session, Error, Step};
use super::super::super::{normalize::target, Warnings};
use super::super::edition;
use rawr_toml::document;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in super::super::super) enum Kind {
    Lib,
    Bin,
    Example,
    Test,
    Bench,
    CustomBuild,
    Metabuild,
}
impl Kind {
    fn human(self) -> &'static str {
        match self {
            Self::Lib => "library",
            Self::Bin => "binary",
            Self::Example => "example",
            Self::Test => "test",
            Self::Bench => "benchmark",
            Self::CustomBuild | Self::Metabuild => "custom build",
        }
    }
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in super::super::super) enum Scrape {
    Unset,
    Enabled,
    Disabled,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in super::super::super) struct Settings {
    tested: bool,
    documented: bool,
    doctested: bool,
    benched: bool,
    harness: bool,
    proc_macro: bool,
    for_host: bool,
    doc_scrape_examples: Scrape,
    edition: edition::Edition,
}
impl Settings {
    pub(in super::super::super) fn tested(self) -> bool {
        self.tested
    }
    pub(in super::super::super) fn documented(self) -> bool {
        self.documented
    }
    pub(in super::super::super) fn doctested(self) -> bool {
        self.doctested
    }
    pub(in super::super::super) fn benched(self) -> bool {
        self.benched
    }
    pub(in super::super::super) fn harness(self) -> bool {
        self.harness
    }
    pub(in super::super::super) fn proc_macro(self) -> bool {
        self.proc_macro
    }
    pub(in super::super::super) fn for_host(self) -> bool {
        self.for_host
    }
    pub(in super::super::super) fn doc_scrape_examples(self) -> Scrape {
        self.doc_scrape_examples
    }
    pub(in super::super::super) fn edition(self) -> edition::Edition {
        self.edition
    }
}

pub(in super::super::super) fn defaults<A: document::Admission>(
    kind: Kind,
    edition: edition::Edition,
    session: &mut Session<'_, A>,
) -> Step<Settings> {
    session.items(1)?;
    let mut settings = Settings {
        tested: true,
        documented: false,
        doctested: false,
        benched: true,
        harness: true,
        proc_macro: false,
        for_host: false,
        doc_scrape_examples: Scrape::Unset,
        edition,
    };
    match kind {
        Kind::Lib => {
            settings.documented = true;
            settings.doctested = true;
        }
        Kind::Bin => settings.documented = true,
        Kind::Example => {
            settings.tested = false;
            settings.benched = false;
        }
        Kind::Test => settings.benched = false,
        Kind::Bench => settings.tested = false,
        Kind::CustomBuild | Kind::Metabuild => {
            settings.for_host = true;
            settings.benched = false;
            settings.tested = false;
            settings.doc_scrape_examples = Scrape::Disabled;
            if kind == Kind::Metabuild {
                let mut ignored = Warnings::new();
                match edition::select(Some("2018"), None, &mut ignored, session)? {
                    Ok(edition) => settings.edition = edition,
                    Err(_) => return session.fail(Error::State),
                }
            }
        }
    }
    Ok(settings)
}

pub(in super::super::super) fn configure<A: document::Admission>(
    original: Option<&Target<'_>>,
    name: &str,
    kind: Kind,
    edition: edition::Edition,
    warnings: &mut Warnings,
    session: &mut Session<'_, A>,
) -> Step<Result<Settings, edition::Failure>> {
    let mut settings = defaults(kind, edition, session)?;
    settings.tested = super::boolean(original, "test", session)?.unwrap_or(settings.tested);
    settings.documented = super::boolean(original, "doc", session)?.unwrap_or(settings.documented);
    settings.doctested =
        super::boolean(original, "doctest", session)?.unwrap_or(settings.doctested);
    settings.benched = super::boolean(original, "bench", session)?.unwrap_or(settings.benched);
    settings.harness = super::boolean(original, "harness", session)?.unwrap_or(settings.harness);
    let proc_macro = super::boolean(original, "proc-macro", session)?.or(super::boolean(
        original,
        "proc_macro",
        session,
    )?);
    settings.proc_macro = proc_macro.unwrap_or(settings.proc_macro);
    settings.doc_scrape_examples = match super::boolean(original, "doc-scrape-examples", session)? {
        None => Scrape::Unset,
        Some(true) => Scrape::Enabled,
        Some(false) => Scrape::Disabled,
    };
    // Target::set_proc_macro and set_kind have no implicit effects on for_host.
    let proc_macro = super::boolean(original, "proc-macro", session)?.or(super::boolean(
        original,
        "proc_macro",
        session,
    )?);
    settings.for_host = proc_macro.unwrap_or(settings.for_host);
    let field = target::field(original, "edition", session)?;
    if let Some(edition) = target::string(field, session)? {
        warnings.push(
            &[
                "`edition` is set on ",
                kind.human(),
                " `",
                name,
                "` which is deprecated",
            ],
            session,
        )?;
        match edition::select(Some(edition.as_str()), None, warnings, session)? {
            Ok(edition) => settings.edition = edition,
            Err(error) => return Ok(Err(error)),
        }
    }
    Ok(Ok(settings))
}

#[cfg(test)]
mod tests;

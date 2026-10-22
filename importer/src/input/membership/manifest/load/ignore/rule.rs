// Copyright the ripgrep contributors. MIT OR Unlicense.
//! ignore 0.4.26 gitignore.rs:447–523, add_line(None, rule) before GlobBuilder.
//! Upstream source: /private/tmp/rawr-p9-membership-inventory-sources/ignore-0.4.26.
use crate::input::membership::manifest::{load::formatted, session::Session, Step};
use rawr_toml::document::Admission;

pub(super) struct Prepared {
    pub(super) original: String,
    pub(super) actual: String,
    pub(super) whitelist: bool,
    pub(super) directory: bool,
}
impl Prepared {
    /// Source Glob::clone for add_line(None, rule): clone the two owned text
    /// fields in declaration order before copying its scalar flags.
    pub(super) fn copy<A: Admission>(&self, session: &mut Session<'_, A>) -> Step<Self> {
        let original = formatted(format_args!("{}", self.original), session)?;
        let actual = formatted(format_args!("{}", self.actual), session)?;
        session.items(1)?;
        Ok(Self {
            original,
            actual,
            whitelist: self.whitelist,
            directory: self.directory,
        })
    }

    pub(super) fn build<A: Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<Result<super::glob::Built, super::glob::Failure>> {
        super::glob::build(&self.actual, session)
    }

    pub(super) fn parse<A: Admission>(
        &self,
        session: &mut Session<'_, A>,
    ) -> Step<Result<super::glob::Parsed, super::glob::Failure>> {
        super::glob::parse(&self.actual, session)
    }
}

pub(super) fn prepare<A: Admission>(
    mut line: &str,
    session: &mut Session<'_, A>,
) -> Step<Option<Prepared>> {
    session.check()?;
    session.items(1)?;
    if line.starts_with('#') {
        return Ok(None);
    }
    if !line.ends_with("\\ ") {
        session.items(line.len())?;
        line = line.trim_end();
    }
    if line.is_empty() {
        return Ok(None);
    }
    let original = formatted(format_args!("{line}"), session)?;
    let mut whitelist = false;
    let mut directory = false;
    let mut absolute = false;
    if line.starts_with("\\!") || line.starts_with("\\#") {
        line = &line[1..];
        absolute = line.starts_with('/');
    } else {
        if line.starts_with('!') {
            whitelist = true;
            line = &line[1..];
        }
        if line.starts_with('/') {
            line = &line[1..];
            absolute = true;
        }
    }
    if line.ends_with('/') {
        directory = true;
        line = &line[..line.len() - 1];
        if line.ends_with('\\') {
            line = &line[..line.len() - 1]
        }
    }
    let mut actual = formatted(format_args!("{line}"), session)?;
    if !absolute {
        session.items(line.len())?;
        if !line.contains('/') && !(actual.starts_with("**/") || actual == "**") {
            actual = formatted(format_args!("**/{actual}"), session)?;
        }
    }
    if actual.ends_with("/**") {
        actual = formatted(format_args!("{actual}/*"), session)?;
    }
    Ok(Some(Prepared {
        original,
        actual,
        whitelist,
        directory,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::membership::manifest::{reference::Account, Stop};

    #[test]
    fn rule_preparation_retains_pinned_escapes_and_directory_semantics() {
        for (input, expected) in [
            ("#comment", None),
            (" \t", None),
            ("name  ", Some(("name", "**/name", false, false))),
            ("name\\ ", Some(("name\\ ", "**/name\\ ", false, false))),
            ("\\!name", Some(("\\!name", "**/!name", false, false))),
            ("\\#name", Some(("\\#name", "**/#name", false, false))),
            ("!/cache/", Some(("!/cache/", "cache", true, true))),
            ("cache\\/", Some(("cache\\/", "**/cache", false, true))),
            ("src/**", Some(("src/**", "src/**/*", false, false))),
            ("**", Some(("**", "**", false, false))),
            ("!", Some(("!", "**/", true, false))),
        ] {
            let mut account = Account;
            let mut session = Session::new(&mut account);
            let rule = prepare(input, &mut session).unwrap();
            assert_eq!(
                rule.as_ref().map(|rule| (
                    rule.original.as_str(),
                    rule.actual.as_str(),
                    rule.whitelist,
                    rule.directory
                )),
                expected
            );
        }
    }
    struct Accounted {
        count: usize,
        refuse: usize,
    }
    impl Admission for Accounted {
        type Error = ();
        fn admit(&mut self, _: rawr_toml::document::Request) -> Result<(), ()> {
            let position = self.count;
            self.count += 1;
            if position == self.refuse {
                Err(())
            } else {
                Ok(())
            }
        }
    }
    #[test]
    fn refusal_stops_every_rule_preparation_stage() {
        let mut complete = Accounted {
            count: 0,
            refuse: usize::MAX,
        };
        prepare("cache/**", &mut Session::new(&mut complete)).unwrap();
        for refuse in 0..complete.count {
            let mut account = Accounted { count: 0, refuse };
            let mut session = Session::new(&mut account);
            assert!(matches!(
                prepare("cache/**", &mut session),
                Err(Stop::Refused)
            ));
            assert!(matches!(session.check(), Err(Stop::Refused)));
            assert_eq!(account.count, refuse + 1);
        }
    }
}

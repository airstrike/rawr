//! Scalar Version's parsed components with offsets into stable borrowed/inline
//! text. No self-reference or reparsing is needed to retain its identifiers.

use std::fmt;
use std::ops::Range;

use rawr_toml::document;

use super::super::super::{scalar, session, value, Diagnostic, Span, Step, Tag};

pub(in super::super::super) struct Version<'d> {
    text: value::Text<'d>,
    core: [u64; 3],
    pre: Range<usize>,
    build: Range<usize>,
}

impl<'d> Version<'d> {
    pub(super) fn parse<A: document::Admission>(
        text: value::Text<'d>,
        trim: bool,
        span: Option<Span>,
        session: &mut session::Session<'_, A>,
    ) -> Step<Self> {
        session.check()?;
        let full = text.as_str();
        let spelling = if trim { full.trim() } else { full };
        let parsed = match scalar::Version::parse(spelling) {
            Ok(value) => value,
            Err(error) => return session.reject(Diagnostic::at(Tag::Scalar(error), span)),
        };
        // Identifiers are suffixes of the parsed spelling. Their boundaries
        // follow directly from the parser's successful component lengths.
        let end = if trim {
            full.trim_end().len()
        } else {
            full.len()
        };
        let build = end - parsed.build().len()..end;
        let pre_end = if parsed.build().is_empty() {
            end
        } else {
            build.start - 1
        };
        let pre = pre_end - parsed.pre().len()..pre_end;
        Ok(Self {
            core: [parsed.major(), parsed.minor(), parsed.patch()],
            text,
            pre,
            build,
        })
    }

    pub(in super::super::super) fn major(&self) -> u64 {
        self.core[0]
    }
    pub(in super::super::super) fn minor(&self) -> u64 {
        self.core[1]
    }
    pub(in super::super::super) fn patch(&self) -> u64 {
        self.core[2]
    }
    pub(in super::super::super) fn pre(&self) -> &str {
        &self.text.as_str()[self.pre.clone()]
    }
    pub(in super::super::super) fn build(&self) -> &str {
        &self.text.as_str()[self.build.clone()]
    }
}

impl fmt::Display for Version<'_> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}.{}.{}",
            self.major(),
            self.minor(),
            self.patch()
        )?;
        if !self.pre().is_empty() {
            write!(formatter, "-{}", self.pre())?;
        }
        if !self.build().is_empty() {
            write!(formatter, "+{}", self.build())?;
        }
        Ok(())
    }
}

//! RequiredExtensionStrategyBuilder::build and its two query methods.
use super::{compile, table, Admission, Buffer, Failure, Session, Step};

pub(in super::super) struct Strategy(table::Compiled);

impl Strategy {
    pub(in super::super) fn build<A: Admission>(
        source: table::Regexes,
        session: &mut Session<'_, A>,
    ) -> Step<Result<Self, Failure>> {
        session.check()?;
        let mut destination = table::Compiled::new();
        let mut groups = source.into_iter(session)?;
        while let Some((extension, texts)) = groups.next(session)? {
            destination.insert_empty(&extension, session)?;
            let mut texts = texts.into_iter(session)?;
            while let Some((index, text)) = texts.next(session)? {
                let regex = match compile::single(&text, session)? {
                    Ok(regex) => regex,
                    Err(error) => return Ok(Err(error)),
                };
                destination.push(&extension, index, regex, session)?;
                // text drops only after insertion, matching the source loop.
            }
        }
        Ok(Ok(Self(destination)))
    }

    /// Borrow the path and already-derived extension from the actual Candidate.
    pub(in super::super) fn is_match<A: Admission>(
        &mut self,
        path: &[u8],
        extension: &[u8],
        session: &mut Session<'_, A>,
    ) -> Step<bool> {
        session.items(1)?;
        if extension.is_empty() {
            return Ok(false);
        }
        let Some(regexes) = self.0.get_mut(extension, session)? else {
            return Ok(false);
        };
        for (_, regex) in regexes {
            session.items(1)?;
            if regex.is_match(path, session)? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    pub(in super::super) fn matches_into<A: Admission>(
        &mut self,
        path: &[u8],
        extension: &[u8],
        matches: &mut Buffer<usize>,
        session: &mut Session<'_, A>,
    ) -> Step<()> {
        session.items(1)?;
        if extension.is_empty() {
            return Ok(());
        }
        if let Some(regexes) = self.0.get_mut(extension, session)? {
            for (index, regex) in regexes {
                session.items(1)?;
                if regex.is_match(path, session)? {
                    matches.push(*index, session)?;
                }
            }
        }
        Ok(())
    }
}

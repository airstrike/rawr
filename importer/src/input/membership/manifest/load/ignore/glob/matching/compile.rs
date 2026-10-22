//! Borrow source-owned regex strings through the actual AST/HIR/Core pipeline.
use super::{formatted, regex, Admission, Buffer, Failure, Session, Step};

pub(super) enum Cause {
    Parse(regex::batch::Failure),
    Translate(regex::batch::TranslationFailure),
    Compile(regex::nfa::Failure),
}

pub(super) fn single<A: Admission>(
    text: &str,
    session: &mut Session<'_, A>,
) -> Step<Result<regex::nfa::Regex, Failure>> {
    let batch = regex::batch::Batch::parse_text(text, session)?;
    let result = finish(batch, regex::nfa::Kind::LeftmostFirst, session)?;
    match result {
        Ok(regex) => Ok(Ok(regex)),
        Err(cause) => {
            let pattern = Some(formatted(format_args!("{text}"), session)?);
            let message = cause.render(session)?;
            Ok(Err(Failure { pattern, message }))
        }
    }
}

pub(super) fn set<A: Admission>(
    texts: Buffer<String>,
    session: &mut Session<'_, A>,
) -> Step<Result<regex::nfa::Regex, Failure>> {
    let batch = regex::batch::Batch::parse_texts(texts.as_slice(), session)?;
    let result = finish(batch, regex::nfa::Kind::All, session)?;
    // The source new_regex_set consumes the string vector. Its strings remain
    // live through compilation, then drop before the caller builds the factory.
    match result {
        Ok(regex) => Ok(Ok(regex)),
        Err(cause) => {
            let message = cause.render(session)?;
            Ok(Err(Failure {
                pattern: None,
                message,
            }))
        }
    }
}

impl Cause {
    fn render<A: Admission>(self, session: &mut Session<'_, A>) -> Step<String> {
        match self {
            Self::Parse(error) => error.render(session),
            Self::Translate(error) => error.render(session),
            Self::Compile(error) => error.render(session),
        }
    }
}

fn finish<A: Admission>(
    batch: Result<regex::batch::Batch<'_>, regex::batch::Failure>,
    kind: regex::nfa::Kind,
    session: &mut Session<'_, A>,
) -> Step<Result<regex::nfa::Regex, Cause>> {
    let batch = match batch {
        Ok(batch) => batch,
        Err(error) => return Ok(Err(Cause::Parse(error))),
    };
    let translated = match batch.translate(session)? {
        Ok(translated) => translated,
        Err(error) => return Ok(Err(Cause::Translate(error))),
    };
    Ok(translated.compile(kind, session)?.map_err(Cause::Compile))
}

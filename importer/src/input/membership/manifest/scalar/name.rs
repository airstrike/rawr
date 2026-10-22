//! Cargo's distinct Unicode name grammars, preserving rejection precedence.

use super::Error;

/// The actual Cargo scalar visitor determines the name grammar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) enum Kind {
    Package,
    Registry,
    PathBase,
    Profile,
    Feature,
}

/// Borrowed text checked under one exact name grammar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct Name<'text> {
    text: &'text str,
    kind: Kind,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rejection {
    Empty,
    LeadingDigit(char),
    Start(char),
    Continue(char),
    ProfileCharacter(char),
    ProfileDebug,
    ProfileBuildOverride,
    ProfileReserved,
    FeatureDependencyPrefix,
    FeatureSlash,
}

impl<'text> Name<'text> {
    pub(crate) fn parse(text: &'text str, kind: Kind) -> Result<Self, Error> {
        let result = match kind {
            Kind::Package => text.split("::").try_for_each(basic),
            Kind::Registry | Kind::PathBase => basic(text),
            Kind::Profile => profile(text),
            Kind::Feature => feature(text),
        };
        result.map_err(|reason| Error::Name { kind, reason })?;
        Ok(Self { text, kind })
    }

    pub(crate) fn as_str(&self) -> &'text str {
        self.text
    }

    pub(crate) fn kind(&self) -> Kind {
        self.kind
    }
}

fn basic(text: &str) -> Result<(), Rejection> {
    if text.is_empty() {
        return Err(Rejection::Empty);
    }
    let mut characters = text.chars();
    if let Some(character) = characters.next() {
        if character.is_ascii_digit() {
            return Err(Rejection::LeadingDigit(character));
        }
        if !(unicode_ident::is_xid_start(character) || character == '_') {
            return Err(Rejection::Start(character));
        }
    }
    for character in characters {
        if !(unicode_ident::is_xid_continue(character) || character == '-') {
            return Err(Rejection::Continue(character));
        }
    }
    Ok(())
}

fn profile(text: &str) -> Result<(), Rejection> {
    if let Some(character) = text
        .chars()
        .find(|character| !character.is_alphanumeric() && *character != '_' && *character != '-')
    {
        return Err(Rejection::ProfileCharacter(character));
    }
    // The only contextual lowercase mapping is Greek sigma; both forms are
    // non-ASCII, so streamed casing has identical comparisons to these words.
    if lowercase(text, "debug") {
        return Err(Rejection::ProfileDebug);
    }
    if lowercase(text, "build-override") {
        return Err(Rejection::ProfileBuildOverride);
    }
    if [
        "build",
        "check",
        "clean",
        "config",
        "fetch",
        "fix",
        "install",
        "metadata",
        "package",
        "publish",
        "report",
        "root",
        "run",
        "rust",
        "rustc",
        "rustdoc",
        "target",
        "tmp",
        "uninstall",
    ]
    .into_iter()
    .any(|reserved| lowercase(text, reserved))
        || text
            .chars()
            .flat_map(char::to_lowercase)
            .take(5)
            .eq("cargo".chars())
    {
        return Err(Rejection::ProfileReserved);
    }
    Ok(())
}

fn lowercase(text: &str, word: &str) -> bool {
    text.chars().flat_map(char::to_lowercase).eq(word.chars())
}

fn feature(text: &str) -> Result<(), Rejection> {
    if text.is_empty() {
        return Err(Rejection::Empty);
    }
    if text.starts_with("dep:") {
        return Err(Rejection::FeatureDependencyPrefix);
    }
    if text.contains('/') {
        return Err(Rejection::FeatureSlash);
    }
    let mut characters = text.chars();
    if let Some(character) = characters.next()
        && !(unicode_ident::is_xid_start(character)
            || character == '_'
            || character.is_ascii_digit())
    {
        return Err(Rejection::Start(character));
    }
    for character in characters {
        if !(unicode_ident::is_xid_continue(character)
            || character == '-'
            || character == '+'
            || character == '.')
        {
            return Err(Rejection::Continue(character));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::lowercase;

    #[test]
    fn streamed_reserved_word_comparison_matches_std_lowercase() {
        for text in [
            "DEBUG",
            "BuIlD-Override",
            "PACKAGE",
            "CARGOΣ",
            "ΣCARGO",
            "Σ",
            "İ",
            "CARGOİ",
            "RuStC",
            "rawr",
            "",
            "農曆",
            "ΟΣ",
            "CARGOX",
            "CARG",
        ] {
            for word in [
                "debug",
                "build-override",
                "package",
                "cargo",
                "rustc",
                "rawr",
                "",
            ] {
                assert_eq!(
                    lowercase(text, word),
                    text.to_lowercase() == word,
                    "{text} / {word}"
                );
            }
            assert_eq!(
                text.chars()
                    .flat_map(char::to_lowercase)
                    .take(5)
                    .eq("cargo".chars()),
                text.to_lowercase().starts_with("cargo")
            );
        }
    }
}

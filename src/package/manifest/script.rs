//! A cargo script's manifest: its frontmatter block (D608 point 6).
//!
//! Resolving a script's dependencies is later work. Until then a script
//! that declares any is refused with an error that says so, and one that
//! declares none runs.

use toml_edit::{Document, Item, TableLike};

use super::{byte_span, Error};
use crate::project::frontmatter;

/// Refuse the script `text` when its frontmatter declares dependencies,
/// for every platform or for some. A file without a frontmatter block, or
/// with one cargo refuses, is left to the parse.
pub(crate) fn check(text: &str) -> Result<(), Error> {
    let Ok(frontmatter::Lead {
        frontmatter: Some(frontmatter),
        ..
    }) = frontmatter::read(text)
    else {
        return Ok(());
    };
    let offset = frontmatter.manifest.start;
    let span = |range: Option<std::ops::Range<usize>>| {
        byte_span(
            text,
            range.map_or(frontmatter.open.clone(), |range| {
                range.start + offset..range.end + offset
            }),
        )
    };
    let document = Document::parse(&text[frontmatter.manifest.clone()]).map_err(|error| Error {
        message: error.message().into(),
        span: span(error.span()),
    })?;
    let table = document.as_table();
    let targets = table
        .get("target")
        .and_then(Item::as_table_like)
        .into_iter()
        .flat_map(|targets| targets.iter())
        .filter_map(|(_, target)| target.as_table_like());
    let declared = std::iter::once(table as &dyn TableLike)
        .chain(targets)
        .find_map(|table| {
            let (key, dependencies) = table.get_key_value("dependencies")?;
            let declares = dependencies
                .as_table_like()
                .is_some_and(|dependencies| !dependencies.is_empty());
            declares.then(|| key.span().or_else(|| dependencies.span()))
        });
    match declared {
        Some(range) => Err(Error {
            message: "this script declares dependencies in its frontmatter. Rawr doesn't \
                      resolve a script's dependencies yet. A script with no dependencies runs."
                .into(),
            span: span(range),
        }),
        None => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::check;

    #[test]
    fn a_script_runs_only_without_dependencies() {
        for runs in [
            "fn main() {}",
            "---\n---\nfn main() {}",
            "---cargo\n[package]\nedition = \"2024\"\n---\nfn main() {}",
            "---cargo\n[dependencies]\n---\nfn main() {}",
            "---cargo\n[dev-dependencies]\nrand = \"0.9\"\n---\nfn main() {}",
        ] {
            assert!(check(runs).is_ok(), "{runs:?}");
        }
        for (refused, line) in [
            (
                "---cargo\n[dependencies]\nrand = \"0.9\"\n---\nfn main() {}",
                2,
            ),
            ("---\ndependencies = { rand = \"0.9\" }\n---\n", 2),
            ("#!/x\n---cargo\ndependencies.rand = \"0.9\"\n---\n", 3),
            (
                "---cargo\n[target.'cfg(unix)'.dependencies]\nlibc = \"0.2\"\n---\n",
                2,
            ),
        ] {
            let error = check(refused).expect_err(refused);
            assert!(
                error.message.contains("declares dependencies"),
                "{refused:?}"
            );
            assert_eq!(error.span.start().line(), Some(line), "{refused:?}");
        }
        let error = check("---cargo\n[package\n---\n").expect_err("bad TOML");
        assert_eq!(error.span.start().line(), Some(2));
    }
}

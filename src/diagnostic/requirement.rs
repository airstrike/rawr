//! rustc's notes on the impls an unmet trait bound needed, as its E0277
//! writes them: the impl the bound selects as a help, then "required for
//! `Vec<Opaque>` to implement `Debug`" for each impl from the innermost
//! out, a run of impls of one trait folded into "N redundant requirements
//! hidden".

use crate::{resolve, Position, Span};

use super::{Section, Source};

/// One note of a chain: at the program's impl, or a line of its own.
enum Entry<'a> {
    Placed(&'a resolve::Requirement, &'a resolve::Site),
    Line(String),
}

/// The notes of `chain`, innermost first, that follow the report's own
/// snippet: its lines before its first section.
pub(super) fn lines(chain: &[resolve::Requirement]) -> Vec<String> {
    if chain
        .last()
        .is_some_and(|outermost| outermost.site.is_some())
    {
        return Vec::new();
    }
    entries(chain)
        .into_iter()
        .map_while(|entry| match entry {
            Entry::Line(line) => Some(line),
            Entry::Placed(..) => None,
        })
        .collect()
}

/// The sections of `chain`, innermost first, for the unmet bound `owner:
/// capability` it ends in, each followed by the lines after it.
pub(super) fn sections(
    chain: &[resolve::Requirement],
    owner: &str,
    capability: &str,
    source: &Source,
) -> Vec<Section> {
    let mut sections = Vec::<Section>::new();
    if let Some(help) = conditional(chain, owner, capability, source) {
        sections.push(help);
    }
    for entry in entries(chain) {
        match entry {
            Entry::Placed(requirement, site) => sections.push(placed(requirement, site, source)),
            // A line before any section is among `lines`.
            Entry::Line(line) => {
                if let Some(section) = sections.pop() {
                    sections.push(section.followed(line));
                }
            }
        }
    }
    sections
}

/// The chain's notes in rustc's order: each impl from the innermost out,
/// and after an impl, the impls of traits already noted folded into a
/// count and the outermost of them.
fn entries(chain: &[resolve::Requirement]) -> Vec<Entry<'_>> {
    let mut entries = Vec::new();
    let mut seen = Vec::new();
    let mut index = 0;
    while let Some(requirement) = chain.get(index) {
        entries.push(entry(requirement));
        if !seen.contains(&&requirement.trait_name) {
            seen.push(&requirement.trait_name);
        }
        let mut last = index;
        while let Some(next) = chain.get(last + 1)
            && seen.contains(&&next.trait_name)
        {
            last += 1;
        }
        let hidden = last - index;
        if hidden > 0 {
            let plural = if hidden == 1 { "" } else { "s" };
            entries.push(Entry::Line(format!(
                "{hidden} redundant requirement{plural} hidden"
            )));
            entries.push(Entry::Line(required(&chain[last])));
        }
        index = last + 1;
    }
    entries
}

fn entry(requirement: &resolve::Requirement) -> Entry<'_> {
    match &requirement.site {
        Some(site) => Entry::Placed(requirement, site),
        None => Entry::Line(required(requirement)),
    }
}

fn required(requirement: &resolve::Requirement) -> String {
    format!(
        "required for `{}` to implement `{}`",
        requirement.ty,
        written_trait(&requirement.trait_name)
    )
}

/// A trait as rustc writes it in a note: `std::fmt::Display`, which the
/// prelude doesn't name, and the others by their names.
fn written_trait(name: &str) -> &str {
    match name {
        "Display" => "std::fmt::Display",
        name => name,
    }
}

/// The note at the program's impl or derive that needed the trait.
fn placed(requirement: &resolve::Requirement, site: &resolve::Site, source: &Source) -> Section {
    let section = Section::note(required(requirement));
    match site {
        resolve::Site::Impl {
            trait_path,
            owner,
            bound,
            ..
        } => section
            .primary(source.clone(), written_path(source, *trait_path), None)
            .primary(source.clone(), *owner, None)
            .secondary(
                source.clone(),
                *bound,
                Some("unsatisfied trait bound introduced here".into()),
            ),
        resolve::Site::Derive {
            derive,
            name,
            parameter,
            ..
        } => section
            .primary(source.clone(), *name, None)
            .secondary(
                source.clone(),
                *derive,
                Some("in this derive macro expansion".into()),
            )
            .secondary(
                source.clone(),
                *parameter,
                Some(
                    format!(
                        "type parameter would need to implement `{}`",
                        written_trait(&requirement.trait_name)
                    )
                    .into(),
                ),
            )
            .helped(format!(
                "consider manually implementing `{}` to avoid undesired bounds",
                written_trait(&requirement.trait_name)
            )),
    }
}

/// rustc's help at the program's impl the bound selected, the outermost
/// of the chain, with the requirement of it that fails.
fn conditional(
    chain: &[resolve::Requirement],
    owner: &str,
    capability: &str,
    source: &Source,
) -> Option<Section> {
    let (outermost, inner) = chain.split_last()?;
    let site = outermost.site.as_deref()?;
    let needed = match inner.last() {
        Some(requirement) => format!(
            "`{}: {}`",
            requirement.ty,
            written_trait(&requirement.trait_name)
        ),
        None => format!("`{owner}: {}`", written_trait(capability)),
    };
    let label = Some(format!("unsatisfied requirement introduced here: {needed}").into());
    let (primary, requirement, written) = match site {
        resolve::Site::Impl {
            header,
            bound,
            written,
            ..
        } => (trimmed_header(source, *header), *bound, written),
        resolve::Site::Derive {
            derive,
            parameter,
            written,
            ..
        } => (*derive, *parameter, written),
    };
    Some(
        Section::help(format!(
            "the trait `{}` is conditionally implemented for `{written}`",
            written_trait(&outermost.trait_name)
        ))
        .primary(source.clone(), primary, None)
        .secondary(source.clone(), requirement, label),
    )
}

/// An impl's header without the `{` that ends it: `impl<T: Debug> Summary
/// for Wrapper<T>`, through its `where` clause.
fn trimmed_header(source: &Source, header: Span) -> Span {
    let text = header.text(source.text()).trim_end();
    let kept = text.strip_suffix('{').unwrap_or(text).trim_end();
    let mut end = header.start();
    for character in kept.chars() {
        if character == '\n' {
            let line = end
                .line()
                .map_or(u16::MAX, |line| u16::try_from(line + 1).unwrap_or(u16::MAX));
            end = Position::new(line, 1);
        } else {
            end.advance();
        }
    }
    Span::new(header.start(), end)
}

/// The path a trait's name ends, as written: `fmt::Debug` of `Debug`.
fn written_path(source: &Source, name: Span) -> Span {
    let (Some(line), Some(column)) = (name.start().line(), name.start().position()) else {
        return name;
    };
    let Some(written) = source.text().split('\n').nth(line.saturating_sub(1)) else {
        return name;
    };
    let before: Vec<char> = written.chars().take(column.saturating_sub(1)).collect();
    let mut start = before.len();
    while start >= 2 && before[start - 1] == ':' && before[start - 2] == ':' {
        let segment = before[..start - 2]
            .iter()
            .rev()
            .take_while(|character| character.is_alphanumeric() || **character == '_')
            .count();
        if segment == 0 {
            break;
        }
        start -= 2 + segment;
    }
    let start = Position::new(
        u16::try_from(line).unwrap_or(u16::MAX),
        u16::try_from(start + 1).unwrap_or(u16::MAX),
    );
    Span::new(start, name.end())
}

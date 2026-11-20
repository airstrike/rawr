//! Where rustc's E0061 marks a call with the wrong number of arguments:
//! each argument the callee doesn't take, the parameters the call gives
//! nothing, those parameters at the callee's definition, and the code
//! that removes or provides them.

use crate::resolve::{Arguments, Missing};
use crate::{ImmutableString, Position, Span};

use super::suggestion::{Edit, Style};
use super::{Error, Source, Suggestion};

/// The labels rustc puts beside the callee: one at each unexpected
/// argument, and one at each run of adjacent missing parameters.
pub(super) fn labels(
    arguments: &Arguments,
    source: &Source,
    end: Position,
) -> Vec<(Span, ImmutableString)> {
    let given = &arguments.given;
    let mut labels: Vec<(Span, ImmutableString)> = arguments
        .unexpected
        .iter()
        .map(|unexpected| {
            // One argument alone has no number.
            let number = if given.len() == 1 {
                String::new()
            } else {
                format!(" #{}", unexpected.index + 1)
            };
            let label = format!("unexpected argument{number} of type `{}`", unexpected.ty);
            (given[unexpected.index], label.into())
        })
        .collect();
    let parentheses = parentheses(source, end, given.first().copied()).0;
    for run in runs(&arguments.missing) {
        let (first, last) = (&run[0], &run[run.len() - 1]);
        // A run's label is at the arguments in its parameters' places,
        // when the call has them, otherwise at the parentheses.
        let at = match (given.get(first.index), given.get(last.index)) {
            (Some(first), Some(last)) => Span::new(first.start(), last.end()),
            _ => parentheses,
        };
        let types: Option<Vec<&str>> = run.iter().map(|missing| missing.ty.as_deref()).collect();
        let label = match (run, types.as_deref()) {
            ([missing], Some([ty])) => {
                format!("argument #{} of type `{ty}` is missing", missing.index + 1)
            }
            ([missing], _) => format!("argument #{} is missing", missing.index + 1),
            ([_, _], Some([first, second])) => {
                format!("two arguments of type `{first}` and `{second}` are missing")
            }
            ([_, _], _) => "two arguments are missing".to_owned(),
            ([_, _, _], Some([first, second, third])) => {
                format!("three arguments of type `{first}`, `{second}`, and `{third}` are missing")
            }
            ([_, _, _], _) => "three arguments are missing".to_owned(),
            _ => "multiple arguments are missing".to_owned(),
        };
        labels.push((at, label.into()));
    }
    labels
}

/// rustc's suggestion: the call without its unexpected arguments, or with
/// a placeholder for each missing one.
pub(super) fn suggestion(
    arguments: &Arguments,
    source: &Source,
    end: Position,
) -> Result<Option<Suggestion>, Error> {
    let given = &arguments.given;
    let mut edits = Vec::new();
    let message = if arguments.unexpected.is_empty() {
        let close = parentheses(source, end, given.first().copied()).1;
        let mut before = 0;
        for run in runs(&arguments.missing) {
            let placeholders = run
                .iter()
                .map(|missing| missing.placeholder.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            // The arguments the parameters before the run take.
            let taken = run[0].index - before;
            before += run.len();
            let (at, inserted) = match given.get(taken) {
                Some(next) => (next.start(), format!("{placeholders}, ")),
                None if taken == 0 => (close, placeholders),
                None => (close, format!(", {placeholders}")),
            };
            edits.push((Span::new(at, at), inserted));
        }
        if arguments.missing.len() == 1 {
            "provide the argument"
        } else {
            "provide the arguments"
        }
    } else {
        let unexpected = |index: usize| {
            arguments
                .unexpected
                .iter()
                .any(|unexpected| unexpected.index == index)
        };
        let mut index = 0;
        while index < given.len() {
            if !unexpected(index) {
                index += 1;
                continue;
            }
            let first = index;
            while index + 1 < given.len() && unexpected(index + 1) {
                index += 1;
            }
            // A run goes with the separator before it, or with the one
            // after it when it starts the arguments.
            let removed = if first > 0 {
                Span::new(given[first - 1].end(), given[index].end())
            } else if let Some(next) = given.get(index + 1) {
                Span::new(given[first].start(), next.start())
            } else {
                Span::new(given[first].start(), given[index].end())
            };
            edits.push((removed, String::new()));
            index += 1;
        }
        if arguments.unexpected.len() == 1 {
            "remove the extra argument"
        } else {
            "remove the extra arguments"
        }
    };
    let mut made = edits
        .into_iter()
        .map(|(span, replacement)| Edit::new(source.clone(), span, replacement));
    let Some(first) = made.next().transpose()? else {
        return Ok(None);
    };
    let mut suggestion = Suggestion::new(message, Style::Verbose, first);
    for edit in made {
        suggestion = suggestion.edit(edit?);
    }
    Ok(Some(suggestion))
}

/// The missing parameters' places at the callee's definition, which
/// rustc marks beside its name.
pub(super) fn declared(arguments: &Arguments) -> impl Iterator<Item = Span> + '_ {
    arguments
        .missing
        .iter()
        .filter_map(|missing| missing.declared)
}

/// The runs of adjacent parameters among `missing`, which rustc labels
/// together.
fn runs(missing: &[Missing]) -> Vec<&[Missing]> {
    let mut runs = Vec::new();
    let mut start = 0;
    for index in 1..=missing.len() {
        if index == missing.len() || missing[index].index != missing[index - 1].index + 1 {
            if start < index {
                runs.push(&missing[start..index]);
            }
            start = index;
        }
    }
    runs
}

/// The parentheses of a call in `source` whose `)`, or whose `()`, is at
/// `end`: from the `(` before its first argument (`first`), or before its
/// `)`, through that `)`, and the `)`'s place.
fn parentheses(source: &Source, end: Position, first: Option<Span>) -> (Span, Position) {
    let characters: Vec<char> = source.text().chars().collect();
    let index = |position: Position| character_index(source.text(), position);
    let mut close = index(end);
    if characters.get(close) == Some(&'(') {
        close += 1;
    }
    let mut open = first.map_or(close, |first| index(first.start()));
    while open > 0 && characters[open - 1].is_whitespace() {
        open -= 1;
    }
    let open = if open > 0 && characters[open - 1] == '(' {
        open - 1
    } else {
        open
    };
    let position = |index: usize| position_at(&characters, index);
    (
        Span::new(position(open), position(close + 1)),
        position(close),
    )
}

/// The index among `text`'s characters of the one at `position`.
fn character_index(text: &str, position: Position) -> usize {
    let line = position.line().unwrap_or(1);
    let column = position.position().unwrap_or(1);
    let before: usize = text
        .split_inclusive('\n')
        .take(line.saturating_sub(1))
        .map(|line| line.chars().count())
        .sum();
    before + column.saturating_sub(1)
}

/// The position of the character at `index` among `characters`.
fn position_at(characters: &[char], index: usize) -> Position {
    let (mut line, mut column) = (1_u16, 1_u16);
    for character in characters.iter().take(index) {
        if *character == '\n' {
            line = line.saturating_add(1);
            column = 1;
        } else {
            column = column.saturating_add(1);
        }
    }
    Position::new(line, column)
}

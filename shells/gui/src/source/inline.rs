//! Values in the code: which binding's value shows at the end of which
//! line near the pause, and which name a column of a line is in.

/// How far above the marked line names are looked for, at most.
const REACH: usize = 40;

/// The longest value shown at a line's end, in characters.
const LONGEST: usize = 28;

/// The lines, from 0, that show values, each with the indices into `names`
/// of the values it shows, in the order the line names them. Each name
/// shows once, on the last line at or above `marked` that names it,
/// looking up to the start of the function `marked` is in.
pub fn placed<'a>(
    line: impl Fn(usize) -> Option<std::borrow::Cow<'a, str>>,
    marked: usize,
    names: &[&str],
) -> Vec<(usize, Vec<usize>)> {
    let mut seen = vec![false; names.len()];
    let mut placed = Vec::new();
    for index in (marked.saturating_sub(REACH)..=marked).rev() {
        let Some(text) = line(index) else {
            continue;
        };
        let mut shown = Vec::new();
        for (_, word) in words(&text) {
            if let Some(at) = names.iter().position(|name| *name == word)
                && !seen[at]
            {
                seen[at] = true;
                shown.push(at);
            }
        }
        if !shown.is_empty() {
            placed.push((index, shown));
        }
        // A function's first line names its parameters, and the lines
        // above it are another function's.
        if index < marked && words(&text).any(|(_, word)| word == "fn") {
            break;
        }
    }
    placed.reverse();
    placed
}

/// `value` as a line's end shows it: on one line, cut short when long.
pub fn short(value: &str) -> String {
    let flat: String = value.split_whitespace().collect::<Vec<_>>().join(" ");
    if flat.chars().count() <= LONGEST {
        return flat;
    }
    let mut cut: String = flat.chars().take(LONGEST - 1).collect();
    cut.push('…');
    cut
}

/// The name `text` has at character `column`, if any.
pub fn word_at(text: &str, column: usize) -> Option<&str> {
    words(text)
        .find(|(start, word)| (*start..*start + word.chars().count()).contains(&column))
        .map(|(_, word)| word)
}

/// The names in `text`, each with the character it starts at: its
/// identifiers outside strings and comments.
fn words(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut found = Vec::new();
    let mut quoted = false;
    let mut escaped = false;
    let mut start: Option<(usize, usize)> = None;
    let mut chars = text.char_indices().enumerate().peekable();
    while let Some((column, (byte, character))) = chars.next() {
        if quoted {
            match (escaped, character) {
                (false, '\\') => escaped = true,
                (false, '"') => quoted = false,
                _ => escaped = false,
            }
            continue;
        }
        if character == '_' || character.is_alphanumeric() {
            start.get_or_insert((column, byte));
            continue;
        }
        if let Some((first, from)) = start.take() {
            found.push((first, &text[from..byte]));
        }
        match (character, chars.peek().map(|(_, (_, next))| *next)) {
            ('/', Some('/')) => break,
            ('"', _) => quoted = true,
            _ => {}
        }
    }
    if let Some((first, from)) = start.filter(|_| !quoted) {
        found.push((first, &text[from..]));
    }
    found
        .into_iter()
        .filter(|(_, word)| !word.starts_with(|first: char| first.is_ascii_digit()))
}

#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use super::{placed, short, word_at};

    const SOURCE: [&str; 6] = [
        "fn other(absent: u32) {}",
        "fn tick(count: u32, point: Point) -> u32 {",
        "    // count is a comment",
        "    let label = \"count\";",
        "    let next = count + point.x;",
        "    next",
    ];

    #[test]
    fn each_value_shows_once_on_the_last_line_near_the_pause_that_names_it() {
        let line = |index: usize| SOURCE.get(index).map(|text| Cow::Borrowed(*text));
        let names = ["count", "point", "label", "next", "absent"];
        assert_eq!(
            placed(line, 4, &names),
            vec![(3, vec![2]), (4, vec![3, 0, 1])],
            "the function's first line ends the search, and comments and strings name nothing"
        );
        assert_eq!(word_at(SOURCE[4], 15), Some("count"));
        assert_eq!(word_at(SOURCE[4], 20), None);
        assert_eq!(word_at(SOURCE[3], 18), None, "a string names nothing");
        assert_eq!(short("Point {\n    x: 1,\n}"), "Point { x: 1, }");
        assert_eq!(short(&"a".repeat(40)).chars().count(), 28);
    }
}

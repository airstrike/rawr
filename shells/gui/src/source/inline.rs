//! Values in the code: which binding's value shows at the end of which
//! line near the pause, and which place a column of a line is in.

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

/// The characters, from 0, of the place `text` has at character `column`:
/// the name there with the fields, tuple items, indices and derefs around
/// it, as `*player.items[turn].0`. `None` where no place is, as in a
/// string, a comment or between tokens.
pub fn place_at(text: &str, column: usize) -> Option<std::ops::Range<usize>> {
    let characters: Vec<char> = text.chars().collect();
    let word = |at: usize| {
        characters
            .get(at)
            .is_some_and(|character| *character == '_' || character.is_alphanumeric())
    };
    let pointed = *characters.get(column)?;
    if !in_code(text, column) {
        return None;
    }
    let (mut start, mut end) = match pointed {
        '[' => (column, closing(&characters, column)? + 1),
        ']' => (matching(&characters, column)?, column + 1),
        '.' | '*' => (column, column + 1),
        _ if word(column) => (column, column + 1),
        _ => return None,
    };
    while start > 0 && word(start - 1) {
        start -= 1;
    }
    while word(end) {
        end += 1;
    }
    // Leftward: the base before each `.`, through its indices.
    while start >= 2
        && characters[start - 1] == '.'
        && (word(start - 2) || characters[start - 2] == ']')
    {
        start -= 1;
        while start > 0 && characters[start - 1] == ']' {
            start = matching(&characters, start - 1)?;
        }
        while start > 0 && word(start - 1) {
            start -= 1;
        }
    }
    while start > 0 && characters[start - 1] == '*' {
        start -= 1;
    }
    // Rightward: `.field`, `.0` and `[..]` after it.
    loop {
        match characters.get(end) {
            Some('.') if word(end + 1) => {
                end += 1;
                while word(end) {
                    end += 1;
                }
            }
            Some('[') => end = closing(&characters, end)? + 1,
            _ => break,
        }
    }
    Some(start..end)
}

/// The `[` the `]` at `close` closes, at the same depth.
fn matching(characters: &[char], close: usize) -> Option<usize> {
    let mut depth = 0usize;
    for at in (0..=close).rev() {
        match characters[at] {
            ']' => depth += 1,
            '[' => {
                depth -= 1;
                if depth == 0 {
                    return Some(at);
                }
            }
            _ => {}
        }
    }
    None
}

/// The `]` that closes the `[` at `open`.
fn closing(characters: &[char], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (at, character) in characters.iter().enumerate().skip(open) {
        match character {
            '[' => depth += 1,
            ']' => {
                depth -= 1;
                if depth == 0 {
                    return Some(at);
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether character `column` of `text` is code: outside its strings and
/// its comment.
fn in_code(text: &str, column: usize) -> bool {
    let mut quoted = false;
    let mut escaped = false;
    let mut characters = text.chars().enumerate().peekable();
    while let Some((at, character)) = characters.next() {
        if at == column {
            return !quoted;
        }
        if quoted {
            match (escaped, character) {
                (false, '\\') => escaped = true,
                (false, '"') => quoted = false,
                _ => escaped = false,
            }
            continue;
        }
        match (character, characters.peek().map(|(_, next)| *next)) {
            ('/', Some('/')) => return false,
            ('"', _) => quoted = true,
            _ => {}
        }
    }
    false
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

    use super::{place_at, placed, short};

    #[test]
    fn a_place_runs_through_its_fields_indices_and_derefs() {
        let line = "    total += *player.items[turn].0 + count; // player.score";
        let at = |column: usize| place_at(line, column).map(|range| &line[range]);
        assert_eq!(at(20), Some("*player.items[turn].0"), "on `items`");
        assert_eq!(at(15), Some("*player.items[turn].0"), "on `player`");
        assert_eq!(at(28), Some("turn"), "an index is a place of its own");
        assert_eq!(at(26), Some("*player.items[turn].0"), "on `[`");
        assert_eq!(at(39), Some("count"));
        assert_eq!(at(10), None, "between tokens");
        assert_eq!(at(50), None, "in a comment");
    }

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
        assert_eq!(
            place_at(SOURCE[4], 15).map(|range| &SOURCE[4][range]),
            Some("count")
        );
        assert_eq!(place_at(SOURCE[4], 20), None);
        assert_eq!(place_at(SOURCE[3], 18), None, "a string names nothing");
        assert_eq!(short("Point {\n    x: 1,\n}"), "Point { x: 1, }");
        assert_eq!(short(&"a".repeat(40)).chars().count(), 28);
    }
}

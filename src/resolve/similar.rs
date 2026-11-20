//! rustc's choice of a name like a missing one, which its errors suggest
//! ("there is a method `go` with a similar name"): an edit distance that
//! counts a transposition as one edit, a score that favours a substring,
//! and the best of the candidates within a third of the name's length.

use std::cmp;

/// The edit distance between `a` and `b`, counted in characters, when it
/// is at most `limit`: insertions, deletions, substitutions and
/// transpositions of two adjacent characters each count one.
fn distance(a: &str, b: &str, limit: usize) -> Option<usize> {
    let mut a = &a.chars().collect::<Vec<_>>()[..];
    let mut b = &b.chars().collect::<Vec<_>>()[..];
    // `b` is the shorter one.
    if a.len() < b.len() {
        std::mem::swap(&mut a, &mut b);
    }
    let least = a.len() - b.len();
    if least > limit {
        return None;
    }
    // A common prefix or suffix costs nothing.
    while let (Some((first_a, rest_a)), Some((first_b, rest_b))) =
        (a.split_first(), b.split_first())
        && first_a == first_b
    {
        a = rest_a;
        b = rest_b;
    }
    while let (Some((last_a, rest_a)), Some((last_b, rest_b))) = (a.split_last(), b.split_last())
        && last_a == last_b
    {
        a = rest_a;
        b = rest_b;
    }
    if b.is_empty() {
        return Some(least);
    }
    let mut before_previous = vec![usize::MAX; b.len() + 1];
    let mut previous = (0..=b.len()).collect::<Vec<_>>();
    let mut current = vec![0; b.len() + 1];
    for i in 1..=a.len() {
        current[0] = i;
        for j in 1..=b.len() {
            let substitution = usize::from(a[i - 1] != b[j - 1]);
            current[j] = cmp::min(
                previous[j] + 1,
                cmp::min(current[j - 1] + 1, previous[j - 1] + substitution),
            );
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                current[j] = cmp::min(current[j], before_previous[j - 2] + 1);
            }
        }
        [before_previous, previous, current] = [previous, current, before_previous];
    }
    let found = previous[b.len()];
    (found <= limit).then_some(found)
}

/// [`distance`] scored so that `a` within `b`, or `b` within `a`, scores
/// low, when the score is at most `limit`: a length difference counts
/// half, unless one is under half the other's length.
pub(crate) fn substring_distance(a: &str, b: &str, limit: usize) -> Option<usize> {
    let (n, m) = (a.chars().count(), b.chars().count());
    let far = n * 2 < m || m * 2 < n;
    let difference = n.abs_diff(m);
    let score = distance(a, b, limit + difference)? - difference;
    let score = if score == 0 && difference > 0 && !far {
        1
    } else if far {
        score + difference
    } else {
        score + difference.div_ceil(2)
    };
    (score <= limit).then_some(score)
}

/// The furthest a candidate's score may be from `name` for rustc to call
/// it similar: a third of its length in bytes, and at least one.
pub(crate) fn reach(name: &str) -> usize {
    cmp::max(name.len(), 3) / 3
}

/// The index among `candidates` of the one rustc suggests for `lookup`: a
/// match but for case, else the best substring score within a third of
/// its length, ties settled by plain edit distance, else one of the same
/// `_`-separated words in another order.
pub(crate) fn best(candidates: &[&str], lookup: &str) -> Option<usize> {
    best_within(candidates, lookup, None, Score::Substring)
}

/// How [`best_within`] scores a candidate.
#[derive(Clone, Copy, PartialEq)]
enum Score {
    Substring,
    Plain,
}

fn best_within(
    candidates: &[&str],
    lookup: &str,
    limit: Option<usize>,
    score: Score,
) -> Option<usize> {
    let upper = lookup.to_uppercase();
    if let Some(found) = candidates
        .iter()
        .position(|candidate| candidate.to_uppercase() == upper)
    {
        return Some(found);
    }
    let mut limit = limit.unwrap_or_else(|| cmp::max(lookup.chars().count(), 3) / 3);
    let mut best = None;
    let mut tied = Vec::new();
    for (index, candidate) in candidates.iter().enumerate() {
        let found = match score {
            Score::Substring => substring_distance(lookup, candidate, limit),
            Score::Plain => distance(lookup, candidate, limit),
        };
        match found {
            Some(0) => return Some(index),
            Some(found) => {
                match score {
                    Score::Substring => {
                        if found < limit {
                            limit = found;
                            tied.clear();
                        }
                        tied.push(index);
                    }
                    Score::Plain => limit = found - 1,
                }
                best = Some(index);
            }
            None => {}
        }
    }
    // A tie is settled by plain edit distance, within the name's length.
    if tied.len() > 1 {
        let names = tied
            .iter()
            .map(|index| candidates[*index])
            .collect::<Vec<_>>();
        best = best_within(&names, lookup, Some(lookup.len()), Score::Plain)
            .map(|chosen| tied[chosen]);
    }
    best.or_else(|| {
        let words = sorted_words(lookup);
        candidates
            .iter()
            .rposition(|candidate| sorted_words(candidate) == words)
    })
}

fn sorted_words(name: &str) -> Vec<&str> {
    let mut words = name.split('_').collect::<Vec<_>>();
    words.sort_unstable();
    words
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_one_edit_away_is_similar() {
        assert_eq!(best(&["go", "make"], "goo"), Some(0));
        assert_eq!(best(&["go", "make"], "g"), Some(0));
        assert_eq!(best(&["go", "make"], "mak"), Some(1));
        assert_eq!(best(&["go", "make"], "zzz"), None);
        // A transposition is one edit.
        assert_eq!(distance("ab", "ba", 1), Some(1));
        // A tie of substring scores goes to the nearer name.
        assert_eq!(
            best(&["capture", "force_capture"], "forced_capture"),
            Some(1)
        );
        assert_eq!(best(&["b_a"], "a_b"), Some(0));
    }
}

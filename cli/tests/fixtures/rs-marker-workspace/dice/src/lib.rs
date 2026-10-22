/// How many sides a die has.
pub fn sides() -> i64 {
    6
}

/// The face a roll of `seed` shows.
pub fn face(seed: i64) -> i64 {
    seed.rem_euclid(sides()) + 1
}

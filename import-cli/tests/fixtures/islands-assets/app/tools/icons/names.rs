//! The names in a list file, quoted.
pub fn quoted(list: &str) -> String {
    std::fs::read_to_string(list)
        .unwrap()
        .lines()
        .map(|name| format!("{name:?}"))
        .collect::<Vec<_>>()
        .join(", ")
}

//! Native: a child of the marked `home`, which calls into it.

pub fn step(visits: usize) -> String {
    format!(
        "wizard {}",
        super::view(&super::State { visits: visits + 1 })
    )
}

pub fn count() -> usize {
    7
}

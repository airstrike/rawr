//! Native: a sibling of the marked `home` that calls into it.
use super::home;

pub fn title(state: &home::State) -> String {
    format!("about {}", home::view(state))
}

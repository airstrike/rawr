//! Native: the app's workspace, whose `screen` holds the marked `home`.
pub mod screen;

pub fn summary(visits: usize) -> String {
    let state = screen::home::State { visits };
    format!("summary {}", screen::home::view(&state))
}

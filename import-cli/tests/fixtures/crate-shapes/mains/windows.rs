// rawr
// A `&mut` a pattern binds fills a crate's `&dyn Window` input, as
// integration's `interface.update(window, ..)` passes the `&mut
// Arc<Window>` its `let Self::Ready { window, .. } = self` binds.
use natives::windows::{handle_of, Surface};
use std::sync::Arc;

struct State {
    window: Arc<Surface>,
}

fn show(state: &mut State) {
    let State { window } = state;
    println!("{}", handle_of(window));
}

fn main() {
    let window = Arc::new(Surface::new(4));
    println!("{}", handle_of(&window));
    let mut state = State { window };
    show(&mut state);
}

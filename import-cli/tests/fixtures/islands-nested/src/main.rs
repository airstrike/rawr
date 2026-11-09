//! Native Rust: the root declares the native `workspace`, whose native
//! `screen` declares the marked `home` beside the native `about` and
//! `other`. Native code calls into `home`, and `home` names the root's
//! `Message`, its parent's private items and its native siblings' and
//! child's items. With `ISLANDS_STOP` set, it then counts ticks through
//! `home` until the file it names exists.
mod workspace;

use workspace::screen;

#[derive(Debug, Clone, PartialEq)]
enum Message {
    Opened(usize),
    Closed,
}

fn main() {
    let mut state = screen::home::State::new();
    state.visits = 2;
    println!("{}", screen::home::view(&state));
    println!("{:?}", screen::home::open(2));
    println!("{:?}", screen::home::open(0));
    println!("{}", screen::about::title(&state));
    println!("{}", workspace::summary(3));
    println!("{}", screen::home::wizard::step(1));
    println!("{}", screen::home::steps());
    let Some(stop) = std::env::var_os("ISLANDS_STOP") else {
        return;
    };
    for tick in 1..3000 {
        if std::path::Path::new(&stop).exists() {
            break;
        }
        println!("tick {tick} {}", screen::home::label(tick));
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
}

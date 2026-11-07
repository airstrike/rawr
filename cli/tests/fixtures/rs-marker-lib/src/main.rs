//! Native: a thin binary that runs the library.
mod rounds;

fn main() {
    let mut game = rs_marker_lib::score::Game::new();
    for hits in rounds::HITS {
        game.play(hits);
    }
    println!("{}", rs_marker_lib::report::line(&game));
}

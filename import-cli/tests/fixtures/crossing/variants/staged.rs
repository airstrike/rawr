// rawr
use events::staged::{self, Stage, Staged};

/// What a stage starts: the program's own type.
struct Board;

/// A play of two stages, hidden behind an `impl Staged` that names none of
/// its associated types.
fn hidden() -> impl Staged + 'static {
    staged::play(|| (Board, vec!["zero".to_owned()])).stages([
        Stage::new("empty", || (Board, Vec::new())),
        Stage::new("full", || (Board, vec!["one".to_owned(), "two".to_owned()])),
    ])
}

/// The stages a borrow of `$play` has, beside those of a play built in the
/// macro's body, whose `stages` builder is its type's own.
macro_rules! counted {
    ($play:expr) => {{
        let given = &$play;
        let lent = given.stages().len();
        let built = staged::play(|| (2_u32, Vec::<String>::new()))
            .stages([Stage::new("built", || (3_u32, Vec::new()))]);
        (lent, Staged::stages(&built).len())
    }};
}

fn main() {
    let play = hidden();
    println!("stages {}", play.stages().len());
    let full = play.stages().iter().find(|stage| stage.name() == "full");
    println!("found {:?}", full.map(|stage| stage.name()));
    println!("started {:?}", staged::started(&play, full));
    let missing = play.stages().iter().find(|stage| stage.name() == "none");
    println!("missing {:?}", staged::started(&play, missing));
    println!("counted {:?}", counted!(play));
}

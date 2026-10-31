// rawr
// iced's canvas aliases in the fixture's shape. `canvas::Cache` passes its
// parameter on to a type only the alias names, with the parameter
// defaulted to the crate's `Renderer` alias. `Geometry` names an associated
// type of that renderer. `Note<'a>` passes a lifetime and a defaulted theme
// on, as iced's `widget::Text` does. `Cache::sketch` gives the renderer's
// associated type, as iced's `Cache::draw` does, and `Action::publish` and
// `Action::and_capture` are named as values, as multitouch names them.

use events::canvas::{self, Cache, Geometry, Note};

#[derive(Default)]
struct Sketch {
    cache: canvas::Cache,
}

struct Board {
    cache: Cache,
    shapes: Vec<Geometry>,
}

fn note(value: i64) -> Note<'static> {
    Note::new(format!("note {value}"))
}

fn main() {
    let sketch = Sketch::default();
    sketch.cache.draw("circle");
    sketch.cache.draw("line");
    println!("sketch {}", sketch.cache.drawn());
    sketch.cache.clear();
    println!("cleared [{}]", sketch.cache.drawn());

    let board = Board {
        cache: Cache::new(),
        shapes: vec![canvas::sketch("arc"), canvas::sketch("hand")],
    };
    board.cache.draw("frame");
    println!("board {}", board.cache.drawn());
    for shape in &board.shapes {
        println!("{}", canvas::describe(shape));
    }
    // A method whose output is its type parameter's associated type.
    let dial = board.cache.sketch("dial");
    let sketched: Vec<Geometry> = vec![dial, sketch.cache.sketch("face")];
    for shape in &sketched {
        println!("{}", canvas::describe(shape));
    }
    println!("board {}", board.cache.drawn());
    // An associated function through the module and the alias, as
    // the_matrix's `canvas::Cache::with_group(group)`.
    let grouped: canvas::Cache = canvas::Cache::new();
    grouped.draw("group");
    println!("grouped {}", grouped.drawn());
    // A crate's function and method named as values where `Option::map`
    // takes a closure.
    let pressed: Option<i64> = Some(7);
    let action = pressed
        .map(canvas::Action::publish)
        .map(canvas::Action::and_capture);
    println!(
        "{}",
        action.map(|action| action.summary()).unwrap_or_default()
    );
    println!("{}", note(3).text());
}

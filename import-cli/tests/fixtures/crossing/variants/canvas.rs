// rawr
// iced's canvas aliases in the fixture's shape. `canvas::Cache` passes its
// parameter on to a type only the alias names, with the parameter
// defaulted to the crate's `Renderer` alias. `Geometry` names an associated
// type of that renderer. `Note<'a>` passes a lifetime and a defaulted theme
// on, as iced's `widget::Text` does.

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
    println!("{}", note(3).text());
}

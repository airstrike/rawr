// rawr
use std::path::Path;

use events::signatures::{self, Coords, Grid};

fn main() {
    println!("area {}", signatures::area((3, 4)));
    println!("swap {:?}", signatures::swap((1, "one".to_string())));
    let text = String::from("hi");
    println!("shout {}", signatures::shout(&text));
    let words = vec!["a".to_string(), "b".to_string()];
    println!(
        "count {} total {}",
        signatures::count(&words),
        signatures::total(&vec![1, 2, 3])
    );
    println!(
        "span {} halves {:?}",
        signatures::span(2..7),
        signatures::halves(10)
    );
    println!("stem {}", signatures::stem(Path::new("dir/file.txt")));
    let boxed = signatures::boxed(4);
    println!("boxed {} unbox {}", boxed, signatures::unbox(Box::new(5)));
    println!("rebox {}", signatures::unbox(boxed));
    let named = "1, 2".parse::<Coords>().unwrap();
    let inferred: Coords = "3,4".parse().unwrap();
    println!("parsed {:?} {:?} {}", named, inferred.x, named == inferred);
    println!("refused {:?}", "12".parse::<Coords>());
    let grid = Grid::new(4);
    println!("grid {} {}", grid[2], grid[0] + grid[3]);
}

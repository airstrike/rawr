// rawr
// image's `ImageBuffer::from_fn(64, 64, |x, y| ..)`: a crate function
// takes an `FnMut` with no `'static` bound, borrowed for the call, so the
// closure may change what it captures and the program reads it after (#715).
// Function pointers take any number of inputs.

use events::grid::{combine, produce, Grid};

fn add(sum: u32, cell: u8) -> u32 {
    sum + cell as u32
}

fn mix(a: i64, b: i64, c: i64) -> i64 {
    a * 100 + b * 10 + c
}

fn seven() -> i64 {
    7
}

fn main() {
    let mut calls = 0;
    let grid = Grid::from_fn(4, 3, |x, y| {
        calls += 1;
        (x * 10 + y) as u8
    });
    println!("{} after {calls} calls", grid.size());
    println!("{} {} {}", grid.get(0, 0), grid.get(3, 1), grid.get(2, 2));

    let mut seen = Vec::new();
    let doubled = grid.map(|cell| {
        seen.push(cell);
        cell * 2
    });
    println!("{} seen, {} at (3, 2)", seen.len(), doubled.get(3, 2));
    println!("sum {}", doubled.fold(0, add));
    println!("{} {}", combine(1, 2, 3, mix), produce(seven));
}

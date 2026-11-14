// rawr
// image's `ImageBuffer::from_fn(64, 64, |x, y| ..)`: a crate function
// takes an `FnMut` with no `'static` bound, borrowed for the call, so the
// closure may change what it captures and the program reads it after (#715).
// Function pointers take any number of inputs. A `#[track_caller]`
// function's location is the program's call (#1560). A borrow inside a
// type argument is lent for the call (#1953), and a builder holds a
// borrowed `?Sized` key (#801). A closure takes a trait object lent for the
// call (#28), and a builder keeps a borrow of a static or of the program's
// value in `Self` (#610). A function or closure is a crate trait's object
// through the crate's impl of the trait for every callable, as iced's
// markdown takes `&Code::highlight` as a `&dyn Highlighter<Code>` (#1981).

use events::grid::{
    boxed_style, combine, described, here, labeled, placed, produce, requested, styled, styles,
    surface_name, tagged, with_surface, Grid, Keyed, Palette, Scale, Scaled, Styled, Styler, Tone,
    DOUBLE,
};

fn add(sum: u32, cell: u8) -> u32 {
    sum + cell as u32
}

fn mix(a: i64, b: i64, c: i64) -> i64 {
    a * 100 + b * 10 + c
}

fn seven() -> i64 {
    7
}

fn describe(grid: &Grid) -> String {
    format!("grid {}", grid.size())
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
    println!("{}", labeled(&doubled, describe));
    let target = String::from("events");
    println!("{}", tagged(&(target.as_str(), "grid", 7)));
    println!("{}", placed(&("located", here())));
    println!("requested {}", requested(&described(Some("four"), 1)));
    let name = String::from("north");
    println!(
        "keyed {}",
        Keyed::new(2).key_ref(name.as_str()).width(5).shown()
    );
    println!("{}", with_surface(|surface| surface_name(surface)));
    let triple = Scale::new(3);
    println!(
        "scaled {} {} {}",
        Scaled::new(5).shown(),
        Scaled::new(5).scale(&DOUBLE).shown(),
        Scaled::new(5).scale(&triple).shown()
    );
    let offset = 5;
    println!(
        "styled {} {} {}",
        styled(&bold, 3),
        styled(
            &|input: u8, palette: &Palette| palette.base() + u32::from(input) + offset,
            1
        ),
        boxed_style(Box::new(bold), 2)
    );
    println!("tones {}", styles(&Toned));
}

fn bold(input: u8, palette: &Palette) -> u32 {
    palette.base() + u32::from(input) * 2
}

struct Toned;

impl Styled for Toned {
    fn styler(&self) -> &dyn Styler<Tone> {
        &Tone::shade
    }
}

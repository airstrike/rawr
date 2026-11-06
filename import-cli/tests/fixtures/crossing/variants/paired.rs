// rawr
// iced's game_of_life: `rows.cartesian_product(columns)` of two let-bound
// `a..=b` ranges, through itertools' blanket `impl<T: Iterator> Itertools
// for T`, which a `use` in the function's body brings into scope. A
// program iterator crosses as a native iterator that pulls from it on each
// `next()`, so a mapped iterator's closure runs as the crate pulls, in
// rustc's order, and the product the crate gives back is an iterator the
// program drives.

#[derive(Debug, Clone, Copy, PartialEq)]
struct Cell {
    i: isize,
    j: isize,
}

fn cluster(cell: Cell) -> impl Iterator<Item = Cell> {
    use events::paired::Paired;

    let rows = cell.i.saturating_sub(1)..=cell.i.saturating_add(1);
    let columns = cell.j.saturating_sub(1)..=cell.j.saturating_add(1);

    rows.cartesian_product(columns).map(|(i, j)| Cell { i, j })
}

fn neighbors(cell: Cell) -> impl Iterator<Item = Cell> {
    cluster(cell).filter(move |candidate| *candidate != cell)
}

fn main() {
    use events::paired::Paired;

    let around = neighbors(Cell { i: 0, j: 5 }).collect::<Vec<_>>();
    println!("{} {:?}", around.len(), around.first());

    // The crate pulls each side as it needs it: a row's item once for
    // each run of the columns, which restart from a clone.
    let rows = (0..2).map(|i| {
        println!("row {i}");
        i * 10
    });
    let columns = ['a', 'b'].into_iter().map(|c| {
        println!("column {c}");
        c
    });
    let mut pairs = rows.cartesian_product(columns);
    println!("made");
    println!("{:?}", pairs.next());
    for (i, c) in pairs {
        println!("{i}{c}");
    }

    // Only what the consumer takes is pulled.
    let firsts = (1..)
        .map(|n| {
            println!("pull {n}");
            n
        })
        .cartesian_product(0..2)
        .take(3)
        .collect::<Vec<_>>();
    println!("{firsts:?}");

    // The crate's own iterator is an iterator too: its product with
    // another, and a crate function over any iterator, take it as itself.
    let triples = (0..2).cartesian_product(5..7).cartesian_product(['x', 'y']);
    for ((a, b), c) in triples {
        print!("{a}{b}{c} ");
    }
    println!();
    println!(
        "{} {}",
        events::paired::length((0..3).cartesian_product(0..2)),
        events::paired::length((0..4).map(|n| n * 2))
    );

    // A chain of adaptors, and a provided method that consumes.
    let evens = (1..=6).filter(|n| n % 2 == 0).chain(10..12);
    println!("{}", evens.tally());
}

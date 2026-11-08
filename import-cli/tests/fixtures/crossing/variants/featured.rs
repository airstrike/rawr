// rawr
// The package's features as Cargo enables them: its manifest's `default`
// enables `x`, which `--no-default-features` turns off. Marked code reads
// them by `cfg!` and `#[cfg]` on a statement, and the native module
// `flagged.rs`, which the tests copy beside it, by `cfg!`.

mod flagged;

fn main() {
    println!("x {}", cfg!(feature = "x"));
    #[cfg(feature = "x")]
    println!("only with x");
    #[cfg(not(feature = "x"))]
    println!("only without x");
    println!("native x {}", flagged::x());
    println!("same {}", events::paired::same(3));
}

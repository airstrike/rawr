// rawr
use natives::spread::prelude::*;
use natives::spread::{self, Mismatch, Shade};

fn main() {
    let shown: Vec<String> = Shade::ALL
        .spread_iter()
        .cloned()
        .map(|shade| shade.to_string())
        .collect();
    println!("{shown:?}");
    let checked: Result<(), Mismatch> = Shade::ALL
        .spread_iter()
        .cloned()
        .map(|shade| {
            spread::check(&shade)?;
            Ok(())
        })
        .collect();
    println!("{checked:?}");
}

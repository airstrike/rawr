// rawr
use natives::spread::prelude::*;
use natives::spread::{self, Mismatch};

fn main() {
    let shown: Vec<String> = spread::shades()
        .cloned()
        .map(|shade| shade.to_string())
        .collect();
    println!("{shown:?}");
    let checked: Result<(), Mismatch> = spread::shades()
        .cloned()
        .map(|shade| {
            spread::check(&shade)?;
            Ok(())
        })
        .collect();
    println!("{checked:?}");
}

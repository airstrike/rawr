// rawr
// iced's clock: `rotation < 180.0` compares a crate's `Degrees` with a
// number. The crate writes `PartialOrd<f32>` and `PartialEq<f32>` with only
// `partial_cmp` and `eq`, so `<` and its kin are std's provided methods,
// and the type is also `PartialOrd` and `PartialEq` with itself.

use events::canvas::Degrees;

fn main() {
    let turn = Degrees(90.0);
    let side = if turn < 180.0 { "left" } else { "right" };
    println!("turn {side}");
    println!(
        "{} {} {} {} {}",
        turn >= 90.0,
        turn > 90.0,
        turn <= 45.0,
        turn == 90.0,
        turn != 90.0
    );
    let other = Degrees(45.0);
    println!("{} {}", other < turn, other == turn);
}

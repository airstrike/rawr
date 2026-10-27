// rawr
// A dependency's `macro_rules!` macros, expanded from its source: `$crate`
// paths into a crate the program does not name and to hidden items,
// repetitions, a macro that calls itself, and a local of the macro's own
// beside the caller's.
use outer::{list, scaled, total};

fn main() {
    let sum = 5;
    let empty = list![];
    let doubled = list![1, 2, 3 + sum,];
    println!("{} {} {}", empty.len(), doubled.len(), doubled.sum());
    let sum = total!(1, 2, sum);
    println!("{sum}");
    println!("{} {}", scaled!(4), outer::scaled!(2, 3 + 1));
    println!("{}", outer::offset!(sum * 2));
    println!("{:?}", outer::written!(0x123));
    println!(
        "{} {}",
        outer::measured!(doubled),
        outer::Measure::measure(&doubled)
    );
}

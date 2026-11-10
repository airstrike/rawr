// rawr
use natives::doubling::{Bag, Counted, Tally, Total};

fn main() {
    println!("{}", Tally(3).doubled().get());
    println!("{}", Bag(vec![1_u8, 2]).doubled().get());
    println!("{}", Counted::<u8>::doubled(Tally(4)).get());
}

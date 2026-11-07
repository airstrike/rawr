// rawr
use natives::again::form::Holder;

fn main() {
    let holder = Holder(vec![1_u32, 2, 3]);
    println!("{}", holder.0.len());
}

// rawr
// A value that keeps a shared and an exclusive borrow of what it was made
// from lives in the block it is bound in, and the encoder it borrows is
// used again once it has dropped, as rustc's borrow ends there (#1973).
use natives::passes::{Encoder, Target};

fn main() {
    let target = Target::new("frame");
    let mut encoder = Encoder::new();
    {
        let mut pass = Target::clear(&target, &mut encoder, 7);
        pass.draw(1);
        pass.draw(2);
    }
    println!("{}", target.name());
    println!("{}", encoder.finish());
}

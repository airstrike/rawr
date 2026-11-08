// rawr
use natives::forwarding::{Drain, Immediate, Ready, Wrap};

fn main() {
    println!("{}", Wrap::new(Drain(0)).depth());
    println!("{}", Immediate::new(Ready(3)).polls());
}

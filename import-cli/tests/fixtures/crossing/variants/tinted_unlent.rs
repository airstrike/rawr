// rawr
// A program's own call of a native method whose output keeps the value it
// is called on and the node it lends, beside a value it reads for the call
// alone, as iced's `overlay` keeps its widget and tree beside the
// renderer: Rawr lends what such a call keeps only where native code asks
// a program's value for it, and refuses this call at check, where rustc
// compiles it.

use events::visits::Lift;
use events::walks::Node;

fn main() {
    let mut lift = Lift("up".into());
    let mut node = Node::new(Vec::new());
    println!("{}", lift.lifted_at(&mut node, &1).len());
}

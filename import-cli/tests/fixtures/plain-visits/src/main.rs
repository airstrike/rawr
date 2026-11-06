// rawr
use visits::{Visit, Visitor};

struct Seven;

impl Visit for Seven {
    fn visit<V: Visitor>(&self, visitor: V) -> V::Out {
        visitor.int(7)
    }
}

fn main() {
    println!("{}", visits::show(Seven));
}

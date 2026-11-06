// rawr
use visits::{Problem, Read, Reader, Visit, Visitor};

struct Seven;

impl Visit for Seven {
    fn visit<V: Visitor>(&self, visitor: V) -> V::Out {
        visitor.int(7)
    }

    fn maybe<V: Visitor>(&self, visitor: Option<V>) -> Option<V::Out> {
        match visitor {
            Some(visitor) => Some(visitor.int(5)),
            None => None,
        }
    }
}

#[derive(Debug)]
struct Pair(i64, i64);

impl<'a> Read<'a> for Pair {
    fn read<R: Reader<'a>>(mut reader: R) -> Result<Self, R::Error> {
        let first = match reader.number() {
            Some(number) => number,
            None => return Err(<R::Error as Problem>::custom("no first")),
        };
        let second = match reader.number() {
            Some(number) => number,
            None => return Err(R::Error::custom("no second")),
        };
        Ok(Pair(first, second))
    }
}

/// The first of `items`, through a crate's re-exports of std's items.
fn first(items: &[i64]) -> visits::reexports::Option<i64> {
    match items.first() {
        visits::reexports::Some(number) => visits::reexports::Some(*number),
        visits::reexports::None => visits::reexports::None,
    }
}

impl std::fmt::Display for Pair {
    fn fmt(&self, f: &mut visits::reexports::Formatter) -> visits::reexports::fmt::Result {
        visits::reexports::Formatter::write_str(f, "a pair")
    }
}

fn main() {
    println!("{:?} {}", first(&[5]), Pair(1, 2));
    println!("{}", visits::show(Seven));
    println!("{}", visits::shown(&Seven));
    println!("{}", visits::read::<Pair>(&[3, 4]));
    println!("{}", visits::read::<Pair>(&[3]));
}

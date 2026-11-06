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

fn main() {
    println!("{}", visits::show(Seven));
    println!("{}", visits::read::<Pair>(&[3, 4]));
    println!("{}", visits::read::<Pair>(&[3]));
}

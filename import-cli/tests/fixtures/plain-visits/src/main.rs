// rawr
use visits::{Expected, FromKeys, Keys, Problem, Read, Reader, Visit, Visitor};

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
            None => return Err(R::Error::short(1, &"two numbers")),
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

/// What the program wanted, as a derive's visitor says.
struct Wanted;

impl Expected for Wanted {
    fn fmt(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("what ")?;
        describe(formatter)
    }
}

/// What the program wants, written into the formatter it is passed.
fn describe(formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
    write!(formatter, "I {}", "wanted")
}

#[derive(Debug)]
struct Counts {
    odd: i64,
    even: i64,
}

impl<'a> FromKeys<'a> for Counts {
    fn from_keys<M: Keys<'a>>(mut keys: M) -> Result<Self, M::Error> {
        // A key: whether its number is odd, as a derive's `__Field` is.
        enum Parity {
            Odd,
            Even,
        }
        impl<'a> Read<'a> for Parity {
            fn read<R: Reader<'a>>(mut reader: R) -> Result<Self, R::Error> {
                match reader.number() {
                    Some(number) if number % 2 == 1 => Ok(Parity::Odd),
                    Some(_) => Ok(Parity::Even),
                    None => Err(R::Error::custom("no key")),
                }
            }
        }
        let mut odd = 0;
        let mut even = 0;
        while let Some(key) = Keys::next_key::<Parity>(&mut keys)? {
            match key {
                Parity::Odd => odd += 1,
                Parity::Even => even += 1,
            }
        }
        Ok(Counts { odd, even })
    }
}

/// The keys `Counts` reads, as a derive's `FIELDS` are.
const FIELDS: &'static [&'static str] = &["odd", "even"];

fn main() {
    println!("{:?} {}", first(&[5]), Pair(1, 2));
    println!("{}", visits::show(Seven));
    println!("{}", visits::shown(&Seven));
    println!("{}", visits::read::<Pair>(&[3, 4]));
    println!("{}", visits::read::<Pair>(&[3]));
    println!("{}", visits::expecting(&"a str"));
    println!("{}", visits::expecting(&visits::Count::new(2)));
    println!("{}", visits::expecting(&Wanted));
    let mut sum = visits::Sum::default();
    visits::tally(&[1, 2, 3], &mut sum);
    println!("{}", sum.total());
    println!("{}", visits::from_keys::<Counts>(&[1, 2, 3]));
    println!("{}", visits::named(FIELDS));
}

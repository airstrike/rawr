// Traits in practice: a default method, Display and Add from std, an
// Iterator with an associated type, From/Into, and trait objects in a Vec.
use std::fmt;
use std::ops::Add;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Money {
    cents: i64,
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "${}.{:02}", self.cents / 100, self.cents % 100)
    }
}

impl Add for Money {
    type Output = Money;
    fn add(self, other: Money) -> Money {
        Money {
            cents: self.cents + other.cents,
        }
    }
}

impl From<f64> for Money {
    fn from(dollars: f64) -> Money {
        Money {
            cents: (dollars * 100.0).round() as i64,
        }
    }
}

trait Item {
    fn name(&self) -> String;
    fn price(&self) -> Money;
    // A default method: every Item gets it, and any may override it.
    fn label(&self) -> String {
        format!("{} ({})", self.name(), self.price())
    }
}

struct Book {
    title: String,
    price: Money,
}

struct Coffee {
    shots: u32,
}

impl Item for Book {
    fn name(&self) -> String {
        self.title.clone()
    }
    fn price(&self) -> Money {
        self.price
    }
}

impl Item for Coffee {
    fn name(&self) -> String {
        format!("coffee x{}", self.shots)
    }
    fn price(&self) -> Money {
        Money::from(2.5)
            + Money {
                cents: 75 * i64::from(self.shots - 1),
            }
    }
    fn label(&self) -> String {
        format!("☕ {}", self.price())
    }
}

// An iterator: the countdown of a receipt's line numbers.
struct Lines {
    next: u32,
    last: u32,
}

impl Iterator for Lines {
    type Item = u32;
    fn next(&mut self) -> Option<u32> {
        if self.next > self.last {
            return None;
        }
        self.next += 1;
        Some(self.next - 1)
    }
}

fn total<I>(items: I) -> Money
where
    I: IntoIterator,
    I::Item: AsRef<dyn Item>,
{
    items
        .into_iter()
        .fold(Money { cents: 0 }, |sum, item| sum + item.as_ref().price())
}

fn main() {
    let cart: Vec<Box<dyn Item>> = vec![
        Box::new(Book {
            title: "The Rust Book".into(),
            price: 39.99.into(),
        }),
        Box::new(Coffee { shots: 2 }),
        Box::new(Coffee { shots: 1 }),
    ];
    let lines = Lines {
        next: 1,
        last: cart.len() as u32,
    };
    for (line, item) in lines.zip(&cart) {
        println!("{line}. {}", item.label());
    }
    println!("total: {}", total(&cart));
}

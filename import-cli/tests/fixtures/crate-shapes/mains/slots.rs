// rawr
use natives::compose::{self, Row, Slot};
use natives::slots;

fn main() {
    let name = String::from("Verified");
    let row: Row<'_, ()> = slots![compose::icon().size(12), "Verified", name];
    println!("{}", row.draw());
    let into: Slot<_> = "into".into();
    let row: Row<()> = Row::with_children([into]);
    println!("{}", row.draw());
    let from: Slot<()> = From::from("from");
    let owned = <Slot<_> as From<_>>::from(String::from("qualified"));
    let row: Row<()> = Row::with_children([from, owned]);
    println!("{}", row.draw());
    let a: Slot<()> = "a".into();
    let b: Slot<()> = From::from("b");
    let row: Row<()> = Row::with_children([a, b]);
    println!("{}", row.draw());
    let lent: Slot<()> = "lent".into();
    let row: Row<'_, ()> = slots![lent, "literal"];
    println!("{}", row.draw());
    let first: Slot<()> = "first".into();
    let second: Slot<()> = "second".into();
    let mut pushed = Vec::new();
    pushed.push(first);
    pushed.push(second);
    let row: Row<()> = Row::with_children(pushed);
    println!("{}", row.draw());
}

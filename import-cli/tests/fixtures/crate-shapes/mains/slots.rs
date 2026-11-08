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
}

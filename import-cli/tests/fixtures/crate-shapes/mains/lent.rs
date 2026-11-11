// rawr
// A literal's `.into()` into a slot that borrows it, where only the row
// the array fills gives the target: the literal is lent at `'static`, as
// rustc's `impl<'a, M> From<&'a str> for Slot<'a, M>` takes it (#1328).
use natives::component;
use natives::compose::{self, Row, Slot};

fn view() -> Row<'static, ()> {
    Row::with_children(["view".into(), Slot::from("slot")])
}

fn main() {
    let row: Row<()> = Row::with_children([Slot::from(component!["a"]), "b".into()]);
    println!("{}", row.draw());
    let wrapped: Slot<()> = compose::wrap(Row::with_children(["w".into()]));
    let row: Row<()> = Row::with_children([wrapped]);
    println!("{}", row.draw());
    println!("{}", view().draw());
}

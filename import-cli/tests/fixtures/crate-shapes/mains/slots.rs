// rawr
use natives::compose::{self, Row, Slot, Theme};
use natives::slots;

struct Mine(u32);

impl<'a> From<Mine> for Slot<'a, ()> {
    fn from(mine: Mine) -> Self {
        Slot::from(format!("mine {}", mine.0))
    }
}

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
    let row: Row<'_, ()> = slots![compose::badge_check().size(12), "Verified"];
    println!("{}", row.draw());
    let row: Row<()> = slots![
        compose::label("own"),
        Mine(7),
        compose::text::<Theme>("plain")
    ];
    println!("{}", row.draw());
    let text: compose::Text<'_, Theme> = compose::text("into");
    let into: Slot<()> = text.into();
    let mine: Slot<()> = Mine(3).into();
    let label: Slot<()> = Slot::from(compose::label("from"));
    let row: Row<()> = Row::with_children([into, mine, label]);
    println!("{}", row.draw());
}

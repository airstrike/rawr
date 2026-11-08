// rawr
use std::borrow::Cow;

use natives::compose::{self, Row, Slot, Theme};
use natives::{badge, component, slots};

struct Mine(u32);

impl<'a, M> From<Mine> for Slot<'a, M> {
    fn from(mine: Mine) -> Self {
        Slot::from(format!("mine {}", mine.0))
    }
}

/// An icon the program makes, generic over the theme as a generated icon
/// module's `badge_check` is: the `From` impl the slot takes it by fixes
/// the theme.
fn check<'a, Theme>() -> compose::Text<'a, Theme> {
    compose::text("check")
}

/// A component crate's plainest view: badges of an icon and a literal in a
/// row, each literal `'static` text the program holds once.
fn badges<'a>() -> Slot<'a, ()> {
    slots![
        badge![compose::icon().size(12), "Verified"].style(badge::Secondary),
        badge!["Bookmark", compose::icon().size(12)].style(badge::Ghost)
    ]
    .spacing(8)
    .into()
}

/// A component crate's badges, as its `badge!` makes them: each a row
/// wrapped in `Slot::new`, its message open until the view's own fixes it.
fn components<'a>() -> Slot<'a, ()> {
    slots![
        component![compose::badge_check().size(12), "Verified"].style(badge::Secondary),
        component!["Bookmark", compose::icon().size(12)].style(badge::Ghost)
    ]
    .spacing(8)
    .into()
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
    let borrowed: Slot<()> = Slot::from(Cow::Borrowed("borrowed"));
    let owned: Slot<()> = Slot::from(Cow::from(String::from("owned")));
    let row: Row<()> = Row::with_children([borrowed, owned]);
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
    let row: Row<()> = Row::with_children([badges()]);
    println!("{}", row.draw());
    let checked: Slot<()> =
        slots![badge![check().size(12), "Verified"].style(badge::Secondary)].into();
    let row: Row<()> = Row::with_children([checked]);
    println!("{}", row.draw());
    let ruled = slots![check().size(12), compose::icon().size(12), compose::rule()];
    println!("{}", ruled.draw());
    let composed: Slot<()> = component![compose::icon().size(12), "Verified"].into();
    let row: Row<()> = Row::with_children([composed]);
    println!("{}", row.draw());
    let row: Row<()> = Row::with_children([components()]);
    println!("{}", row.draw());
    let row: Row<()> = Row::with_children([Slot::from(component!["a"]), Slot::from("b")]);
    println!("{}", row.draw());
    let wrapped: Slot<()> = compose::wrap(Row::with_children([Slot::from("w")]));
    let row: Row<()> = Row::with_children([wrapped]);
    println!("{}", row.draw());
    let mut many = Vec::new();
    for _ in 0..1000 {
        let view: Slot<()> = slots![
            badge![compose::icon().size(12), "Verified"].style(badge::Secondary),
            badge!["Bookmark", compose::icon().size(12)].style(badge::Ghost)
        ]
        .spacing(8)
        .into();
        many.push(view);
    }
    let row: Row<()> = Row::with_children(many);
    println!("{}", row.draw().len());
}

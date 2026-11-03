// rawr
use events::shelf::{self, Item, Show};

/// A panel that keeps a result across calls.
struct Panel<V> {
    view: V,
    calls: u32,
}

impl<V: Show> Panel<V> {
    fn render(&mut self) -> String {
        self.calls += 1;
        format!("{} {}", self.calls, self.view.show())
    }
}

/// A program's own state that lends its items as `&self.items`.
struct Shelf {
    items: Vec<Item>,
}

impl Shelf {
    fn render(&self) -> String {
        shelf::view(&self.items).show()
    }

    fn names(&self) -> String {
        shelf::names(&self.items)
    }
}

fn main() {
    let mut items = vec![Item::new("first"), Item::new("second")];
    let view = shelf::view(&items);
    println!("view {}", view.show());
    println!("again {}", view.show());
    // Reading the items while the view lives is a second shared borrow.
    println!("names {}", shelf::names(&items));
    // An opaque result may run code when it drops, so the items stay
    // borrowed until it does.
    drop(view);
    items.push(Item::new("third"));
    {
        let scoped = shelf::view(&items);
        println!("scoped {}", scoped.show());
    }
    items.push(Item::new("more"));
    println!("pushed {}", shelf::names(&items));

    // A result invariant in its lifetime.
    let picked = shelf::pick(&items);
    println!("picked {} {}", picked.show(), picked.show());
    drop(picked);
    items.push(Item::new("fourth"));

    // A result kept in a struct field and used across calls.
    let mut panel = Panel {
        view: shelf::view(&items),
        calls: 0,
    };
    println!("panel {}", panel.render());
    println!("panel {}", panel.render());
    drop(panel);
    items.push(Item::new("fifth"));

    // The same values throughout: each counts every show.
    println!("shown {}", shelf::names(&items));

    let state = Shelf { items };
    println!("state {}", state.render());
    println!("state {}", state.render());
    println!("counted {}", state.names());
}

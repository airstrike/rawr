// rawr
use std::rc::Rc;
use std::sync::Arc;

use events::pointers::{self, Config};

fn main() {
    let first = pointers::shared();
    let second = pointers::shared();
    println!(
        "shared {} {} {}",
        Arc::strong_count(&first),
        Arc::ptr_eq(&first, &second),
        first.name()
    );
    let made = pointers::config("made");
    let copy = Arc::clone(&made);
    println!(
        "made {} {}",
        Arc::strong_count(&made),
        pointers::same(&made, &copy)
    );
    let kept = pointers::keep(copy);
    println!(
        "kept {} {} {}",
        Arc::ptr_eq(&made, &kept),
        Arc::strong_count(&kept),
        kept.level()
    );
    println!("level {}", pointers::level(kept));
    println!("after {} {:?}", Arc::strong_count(&made), made);
    let own = Arc::new(Config::new("own", 7));
    let other = own.clone();
    println!(
        "own {} {}",
        pointers::same(&own, &other),
        Arc::strong_count(&own)
    );
    let third = own.clone();
    println!("held {}", pointers::held(third));
    let back = pointers::keep(other);
    println!(
        "back {} {} {}",
        Arc::ptr_eq(&own, &back),
        Arc::strong_count(&own),
        back.name()
    );
    let node = pointers::node("leaf");
    let copied = Rc::clone(&node);
    println!("node {} {}", pointers::describe(&node), copied.label());
    println!("holders {}", pointers::holders(Rc::clone(&node)));
    drop(copied);
    println!("dropped {}", Rc::strong_count(&node));
}

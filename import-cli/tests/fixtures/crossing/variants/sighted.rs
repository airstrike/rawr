// rawr
// iced_test's `simulator(counter.view())` beside iced (#1680): `tester`
// takes any `parts` widget at a theme and renderer no impl it can name
// fixes, and the view's `impl Shown<Message>`, whose blanket impl passes
// its arguments on to `parts`'s trait, fixes them, the trait's defaults,
// as rustc reads the view's opaque type by its bound alone.

use events::shown::{badge, Shown};

#[derive(Debug, Clone)]
enum Message {}

struct Counter {
    value: i64,
}

impl Counter {
    fn view(&self) -> impl Shown<Message> {
        badge(&format!("{}", self.value))
    }
}

fn title() -> impl Shown<Message> {
    badge("Counter")
}

fn main() {
    let counter = Counter { value: 3 };
    let sight = tester::sight(counter.view());
    println!("sight {}", sight.shown());
    let sight = tester::sight(title());
    println!("sight {}", sight.shown());
}

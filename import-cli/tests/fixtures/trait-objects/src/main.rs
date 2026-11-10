// rawr
// A crate's trait objects (D872): a static's place held as `&'static dyn`,
// numbers, a `bool` and crate values lent as `&dyn`, a `Box<dyn>` given
// and taken back, and tracing's callsite, whose metadata holds it as an
// `Identifier(&'static dyn Site)` built at the call. An object's methods
// called from marked code (#1593), and `Rc<dyn>` and `Arc<dyn>` the program
// makes (#1304).

use std::rc::Rc;
use std::sync::Arc;

fn main() {
    static SQUARE: shapes::Square = shapes::Square { side: 3 };
    let held = shapes::Held(&SQUARE);
    println!("held {} {}", held.area(), held.holds(&SQUARE));
    println!("static {}", shapes::area_of(&SQUARE));
    let n = 5i64;
    println!("int {}", shapes::area_of(&n as &dyn shapes::Shape));
    let small = 7u8;
    let object: &dyn shapes::Shape = &small;
    println!("u8 {}", shapes::area_of(object));
    println!("bool {}", shapes::area_of(&true));
    let square = shapes::Square { side: 2 };
    println!("square {}", shapes::area_of(&square));
    println!("boxed {}", shapes::boxed_area(shapes::boxed(4)));
    println!("made {}", shapes::boxed_area(Box::new(6i64)));
    // Counted objects of a crate's trait (#1304).
    let counted = Rc::new(shapes::Square { side: 5 }) as Rc<dyn shapes::Shape>;
    println!("counted {:?}", shapes::counted_area(counted));
    let shared: Arc<dyn shapes::Shape> = Arc::new(7i64);
    println!("shared {}", shapes::shared_area(shared));
    for _ in 0..2 {
        println!("{}", shapes::event!("tick"));
    }
}

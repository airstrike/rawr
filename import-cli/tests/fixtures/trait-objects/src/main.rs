// rawr
// A crate's trait objects (D872): a static's place held as `&'static dyn`,
// numbers, a `bool` and crate values lent as `&dyn`, a `Box<dyn>` given
// and taken back, and tracing's callsite, whose metadata holds it as an
// `Identifier(&'static dyn Site)` built at the call.

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
    for _ in 0..2 {
        println!("{}", shapes::event!("tick"));
    }
}

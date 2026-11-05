// rawr
// iced's sierpinski_triangle, arc, clock and multitouch in the fixture:
// the crate's `Point` shares its name with a type of another crate it
// reaches only through a signature, as iced's `Point` does tiny_skia's.
// The program reads the crate's point's fields, destructures it and
// builds one (#954).

use events::Point;

fn main() {
    let point = Point { x: 1.5, y: 2.0 };
    let Point { x, y } = point;
    let soft = events::soft(point);
    println!("{} {} {x} {y} {} {}", point.x, point.y, soft.x, soft.y);
}

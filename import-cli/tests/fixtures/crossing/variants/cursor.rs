// rawr
// iced's bezier_tool in the fixture: a `Copy` cursor's by-value method
// gives an `Option` of a point whose coordinate type has a default, read
// directly, through `?` and in a closure native code calls with the cursor.

use events::plane::{self, Cursor, Point, Rectangle};

fn inside(cursor: Cursor, bounds: Rectangle) -> Option<Point> {
    let position = cursor.position_in(bounds)?;
    Some(Point::new(position.x * 2.0, position.y))
}

fn main() {
    let bounds = Rectangle {
        x: 10.0,
        y: 20.0,
        width: 100.0,
        height: 50.0,
    };
    let cursor = Cursor::Available(Point::new(15.0, 30.0));
    if let Some(position) = cursor.position_in(bounds) {
        println!("{} {}", position.x, position.y);
    }
    println!("{:?}", inside(cursor, bounds));
    println!("{:?}", inside(Cursor::Unavailable, bounds));
    let hovered = plane::hover(cursor, |cursor| {
        let position = cursor.position_in(bounds)?;
        Some(position.x + position.y)
    });
    println!("{hovered:?}");
}

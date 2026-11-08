// rawr
use natives::arrays::{corners, fill, sum, Pixel, Row, Slot};

fn main() {
    let row = Row::with_children([Slot::new(1), Slot::new(2), Slot::new(3)]);
    println!("{}", row.describe());
    let pair = [Slot::new("a"), Slot::new("b")];
    println!("{}", Row::with_children(pair).describe());

    let mut pixel = Pixel([1, 2, 3, 4]);
    fill(&mut pixel.0, 7);
    println!("{:?}", pixel.0);
    let mut channels = [0u8; 4];
    fill(&mut channels, 9);
    println!("{:?}", channels);

    let values = [10, 20, 30];
    println!("{} {}", sum(&[1, 2, 3]), sum(&values));

    let corners = corners();
    println!("{:?} {}", corners, corners[3]);
}

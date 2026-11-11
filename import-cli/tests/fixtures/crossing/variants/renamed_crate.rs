// rawr
// A dependency crate named by a leading `::` path beside a name of the
// program's own that the crate's name would otherwise mean, as iced's
// screenshot writes `use ::image as img;` and `use ::image::ColorType;`
// beside `iced::widget::image`: the leading `::` names the crate.
use events::image as parts;

use ::events as ev;
use ::parts as soft;
use ::parts::Point;

fn main() {
    let handle = parts::Handle::from_name("shot");
    let point: Point = soft::Point { x: 1.0, y: 2.0 };
    let same = point == ::parts::Point { x: 1.0, y: 2.0 };
    println!("{:?} {:?} {same} {}", handle, point, ev::summed(2u8, 3u16));
}

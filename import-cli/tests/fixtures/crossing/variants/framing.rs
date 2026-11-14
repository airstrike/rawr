// rawr
use events::framing::{self, Gallery, Shape};

struct Wall<'a> {
    label: &'a str,
}

impl<'a> Gallery<'a> for Wall<'a> {
    fn shape(&self, side: u32) -> impl Shape<u32> + 'a {
        let shape = framing::square(side);
        let caption = framing::caption(self.label);
        framing::framed(shape, caption)
    }
}

fn main() {
    let label = String::from("oak");
    let wall = Wall { label: &label };
    println!("{}", framing::show(&wall, 2));
    println!("{}", framing::show(&wall, 4));
}

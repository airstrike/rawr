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

    fn outline(&self, label: &'a str) -> impl Shape<u32> + 'a {
        let shape = framing::traced(label);
        framing::framed(shape, framing::caption(label))
    }

    fn lines(&self, parts: &'a [framing::Part]) -> impl Shape<u32> + 'a {
        let shape = framing::lined(self, parts, tagged);
        framing::framed(shape, framing::caption(self.label))
    }
}

fn tagged(name: String) -> String {
    format!("#{name}")
}

fn main() {
    let label = String::from("oak");
    let wall = Wall { label: &label };
    println!("{}", framing::show(&wall, 2));
    println!("{}", framing::show(&wall, 4));
    let plan = framing::Plan::new(vec!["beam".to_owned(), "post".to_owned()]);
    let listing = framing::listing(plan.parts(), &wall);
    println!("{}", listing.render());
}

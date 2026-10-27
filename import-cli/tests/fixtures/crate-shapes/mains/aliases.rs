// rawr
use natives::Element;

fn first(message: i64) -> Element<'static, i64> {
    Element::new(message)
}

pub fn main() -> natives::Result {
    let element = first(7);
    println!("message {}", element.message());
    println!("{:?}", natives::run(false));
    natives::run(true)
}

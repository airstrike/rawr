// rawr
use natives::text;

fn main() {
    let label = text("count").size(20);
    println!("{}", label.describe());
    let other: text::Text = text::Text::new("other");
    println!("{}", other.describe());
    println!("{}", natives::text("full").describe());
}

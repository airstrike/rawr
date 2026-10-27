// rawr
use natives::widget::{text, Space};

fn main() {
    let label = text("count").size(20);
    println!("{}", label.describe());
    let other: text::Text = text::Text::new("other");
    println!("{}", other.describe());
    println!("{}", natives::widget::text("full").describe());
    println!("{}", natives::base::text("base").describe());
    println!("{}", Space { width: 1 }.width);
}

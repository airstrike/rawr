// rawr
use natives::widget::{Space, Widget};

fn main() {
    let boxed = Space { width: 4 }.boxed();
    println!("{}", boxed.width);
    let through = Widget::boxed(Space { width: 5 });
    println!("{}", through.width);
    let qualified = natives::widget::Widget::boxed(Space { width: 6 });
    println!("{qualified:?}");
}

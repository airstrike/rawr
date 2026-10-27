// rawr
use natives::widget::{Boxed, Space, Widget};

fn main() {
    let boxed: Boxed<'static, i64> = Space { width: 4 }.boxed();
    println!("{}", boxed.width());
    let through: Boxed<'static, i64> = Widget::boxed(Space { width: 5 });
    println!("{}", through.width());
    let qualified: Boxed<'static, i64> = natives::widget::Widget::boxed(Space { width: 6 });
    println!("{}", qualified.width());
}

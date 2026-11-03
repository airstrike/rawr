// rawr
use events::nested::b;
use events::nested::widget::operation;

fn main() {
    println!("f {}", b::c::f(1));
    println!("g {}", b::c::d::g());
    println!("through {}", operation::c::f(41));
    println!("deeper {}", operation::c::d::g());
    let f = operation::c::f;
    println!("named {}", f(2));
}

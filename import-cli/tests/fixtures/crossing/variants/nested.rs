// rawr
use events::nested::b;
use events::nested::widget::operation;

/// A type through the module, in a signature.
fn counted(count: &mut operation::c::Count) -> u32 {
    count.next()
}

fn main() {
    println!("f {}", b::c::f(1));
    println!("g {}", b::c::d::g());
    println!("through {}", operation::c::f(41));
    println!("deeper {}", operation::c::d::g());
    let mut count: b::c::Count = operation::c::Count::new(9);
    println!("count {} {}", counted(&mut count), counted(&mut count));
}

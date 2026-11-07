// rawr
use natives::form::{countdown, Holder};

fn main() {
    let mut steps = countdown(3);
    println!("{:?}", steps.next());
    for step in steps {
        println!("step {step}");
    }
    println!("{}", countdown(4).count());
    let doubled: Vec<u32> = countdown(3).map(|step| step * 2).collect();
    println!("{doubled:?}");
    let mut total = 0;
    for item in Holder(vec![1_u32, 2, 3]).into_items() {
        total += item;
    }
    let big = Holder(vec![1_u32, 2, 3]).into_items().filter(|item| *item > 1).count();
    println!("{total} {big}");
}

struct Bag {
    items: Vec<i32>,
    count: i32,
}

fn fill(bag: &mut Bag) {
    let mut add = |x| {
        bag.items.push(x);
        bag.count += 1;
    };
    add(4);
    add(5);
}

fn main() {
    let mut bag = Bag { items: vec![], count: 0 };
    let __b = &mut bag;
    for __i in 0..2 {
        __touch(__b);
        fill(&mut bag);
    }
    println!("{:?} {}", bag.items, bag.count);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

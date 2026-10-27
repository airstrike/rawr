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
    fill(&mut bag);
    println!("{:?} {}", bag.items, bag.count);
}

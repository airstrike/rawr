struct Bag {
    items: Vec<i32>,
    label: String,
}

fn main() {
    let mut bags = vec![Bag { items: vec![], label: String::from("a") }];
    for bag in bags.iter_mut() {
        match bag {
            Bag { items, label } => {
                let mut add = |x| items.push(x);
                let __b = &mut add;
                add(1);
                if __c() {
                    __touch(__b);
                }
                add(2);
                label.push('!');
            }
        }
    }
    println!("{:?} {}", bags[0].items, bags[0].label);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

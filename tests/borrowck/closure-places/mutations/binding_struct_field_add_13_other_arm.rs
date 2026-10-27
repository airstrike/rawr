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
                add(1);
                let __b = &mut add;
                if __c() {
                    __touch(__b);
                } else {
                    add(2);
                }
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

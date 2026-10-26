struct View<'a> {
    value: &'a i32,
    count: i32,
}

fn main() {
    let mut a = 1;
    let view = View { value: &a, count: 0 };
    let v = view.value;
    a += 1;
    println!("{v} {}", view.count);
}

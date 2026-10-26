struct View<'a> {
    value: &'a i32,
}

fn main() {
    let mut a = 1;
    let view = View { value: &a };
    let moved = view;
    a += 1;
    println!("{}", moved.value);
}

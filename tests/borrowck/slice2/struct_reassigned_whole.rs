struct View<'a> {
    value: &'a i32,
}

fn main() {
    let mut a = 1;
    let b = 2;
    let mut view = View { value: &a };
    println!("{}", view.value);
    view = View { value: &b };
    a += 1;
    println!("{} {a}", view.value);
}

struct Holder<'a> {
    value: &'a i32,
}

impl<'a> Holder<'a> {
    fn new(value: &'a i32) -> Self {
        Holder { value }
    }
}

fn main() {
    let mut x = 1;
    let h = Holder::new(&x);
    x += 1;
    println!("{}", h.value);
}

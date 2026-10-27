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
    println!("{}", h.value);
    let __t = vec![&x].to_vec();
    __see(&__t);
    x += 1;
    println!("{x}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

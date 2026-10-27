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
    x += 1;
    let __t = if __c() { &__id(&x) } else { &__id(&x) };
    println!("{x}");
    __see(__t);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

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
    let __v = vec![&x];
    x += 1;
    __see(&__v);
    println!("{x}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);

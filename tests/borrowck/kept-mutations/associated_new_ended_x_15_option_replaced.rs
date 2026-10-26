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
    let mut __o = Some(&x);
    __see(&__o);
    __o = None;
    x += 1;
    __see(&__o);
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

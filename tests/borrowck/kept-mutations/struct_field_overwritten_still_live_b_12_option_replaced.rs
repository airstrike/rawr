struct Pair<'a> {
    left: &'a i32,
    right: &'a i32,
}

fn main() {
    let a = 1;
    let mut b = 2;
    let c = 3;
    let mut pair = Pair { left: &a, right: &b };
    pair.right = &c;
    let mut __o = Some(&b);
    __see(&__o);
    __o = None;
    b += 1;
    __see(&__o);
    println!("{} {} {b}", pair.left, pair.right);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);

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
    let __k = __keep(&b);
    __see(__k);
    b += 1;
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

struct Pair<'a> {
    left: &'a i32,
    right: &'a i32,
}

fn main() {
    let a = 1;
    let mut b = 2;
    let c = 3;
    let mut pair = Pair { left: &a, right: &b };
    let __k = __keep(&pair);
    for __i in 0..2 {
        __see(__k);
        pair.right = &c;
    }
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

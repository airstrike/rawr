struct Pair<'a> {
    left: &'a i32,
    right: &'a i32,
}

fn main() {
    let a = 1;
    let mut b = 2;
    let c = 3;
    let mut pair = Pair { left: &a, right: &b };
    let __t = if __c() { &__id(&pair) } else { &__id(&pair) };
    pair.right = &c;
    __see(__t);
    b += 1;
    println!("{} {} {b}", pair.left, pair.right);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

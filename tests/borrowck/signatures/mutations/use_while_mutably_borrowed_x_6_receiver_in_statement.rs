fn main() {
    let mut x = 5;
    let r = &mut x;
    let y = x;
    *r += 1;
    __see(&vec![&x].first());
    println!("{} {}", x, y);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

#[derive(Clone, Copy)]
struct P {
    x: i32,
    y: i32,
}

fn main() {
    let mut p = P { x: 1, y: 2 };
    let mut c = move || {
        p.x += 10;
        p.x + p.y
    };
    let mut d = c;
    let mut e = d;
    println!("{} {} {} {}", c(), d(), e(), c());
    let __b = &mut p;
    p.y = 5;
    if __c() {
        __touch(__b);
    }
    println!("{} {}", p.x, p.y);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

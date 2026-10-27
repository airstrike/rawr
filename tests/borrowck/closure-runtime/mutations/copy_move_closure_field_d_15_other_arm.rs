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
    let __b = &mut d;
    if __c() {
        __touch(__b);
    } else {
        println!("{} {} {} {}", c(), d(), e(), c());
    }
    p.y = 5;
    println!("{} {}", p.x, p.y);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

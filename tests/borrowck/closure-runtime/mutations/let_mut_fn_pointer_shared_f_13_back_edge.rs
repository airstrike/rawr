fn double(x: i32) -> i32 {
    x * 2
}

fn triple(x: i32) -> i32 {
    x * 3
}

fn main() {
    let mut f: fn(i32) -> i32 = double;
    let g = &f;
    println!("{} {}", f(1), g(2));
    let __b = &mut f;
    for __i in 0..2 {
        __touch(__b);
        f = triple;
    }
    println!("{}", f(1));
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

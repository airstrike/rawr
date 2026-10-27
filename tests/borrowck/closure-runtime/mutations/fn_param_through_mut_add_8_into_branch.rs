fn call(f: &mut impl Fn(i32) -> i32) -> i32 {
    f(2) + f(3)
}

fn main() {
    let base = 10;
    let mut add = |x| x + base;
    let __b = &mut add;
    println!("{}", call(&mut add));
    if __c() {
        __touch(__b);
    }
    println!("{}", add(1));
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

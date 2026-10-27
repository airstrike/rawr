fn call(f: &mut impl Fn(i32) -> i32) -> i32 {
    f(2) + f(3)
}

fn main() {
    let base = 10;
    let mut add = |x| x + base;
    for __i in 0..2 {
        let __b = &mut add;
        __touch(__b);
        println!("{}", call(&mut add));
    }
    println!("{}", add(1));
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

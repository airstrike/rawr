fn call(f: &mut impl Fn(i32) -> i32) -> i32 {
    f(2) + f(3)
}

fn main() {
    let base = 10;
    let mut add = |x| x + base;
    println!("{}", call(&mut add));
    let __s = &add;
    println!("{}", add(1));
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

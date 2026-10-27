fn call_twice(mut f: impl FnMut() -> i32) -> i32 {
    f();
    f()
}

fn main() {
    let mut count = 10;
    let mut c = move || {
        count += 1;
        count
    };
    println!("{}", call_twice(c));
    println!("{}", c());
    println!("{}", call_twice(c));
    let __b = &mut count;
    for __i in 0..2 {
        __touch(__b);
        println!("{count}");
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

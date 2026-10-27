fn main() {
    let mut count = 0;
    let mut c = move || {
        count += 1;
        count
    };
    println!("{}", c());
    let mut __f = || __touch(&mut count);
    let mut __g = __f;
    __g();
    __f();
    println!("{} {}", c(), count);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

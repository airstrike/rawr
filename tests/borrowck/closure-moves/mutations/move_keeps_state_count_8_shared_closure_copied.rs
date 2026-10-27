fn main() {
    let mut count = 0;
    let mut c = move || {
        count += 1;
        count
    };
    println!("{}", c());
    let __f = || __see(&count);
    let __g = __f;
    __f();
    println!("{} {}", c(), count);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

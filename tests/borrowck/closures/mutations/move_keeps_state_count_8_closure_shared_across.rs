fn main() {
    let mut count = 0;
    let mut c = move || {
        count += 1;
        count
    };
    println!("{}", c());
    let __g = || __see(&count);
    println!("{} {}", c(), count);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

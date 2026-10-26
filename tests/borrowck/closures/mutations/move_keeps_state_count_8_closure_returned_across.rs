fn main() {
    let mut count = 0;
    let mut c = move || {
        count += 1;
        count
    };
    println!("{}", c());
    let __h = || &count;
    let __k = __h();
    println!("{} {}", c(), count);
    __see(__k);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

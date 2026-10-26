fn main() {
    let mut count = 0;
    let mut c = move || {
        count += 1;
        count
    };
    println!("{}", c());
    let mut __f = || __touch(&mut count);
    for __i in 0..2 {
        __f();
        println!("{} {}", c(), count);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

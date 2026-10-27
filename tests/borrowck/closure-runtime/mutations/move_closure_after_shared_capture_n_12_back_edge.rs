fn main() {
    let mut n = 1;
    {
        let show = || println!("{n}");
        show();
    }
    let mut c = move || {
        n += 1;
        n
    };
    println!("{} {}", c(), c());
    let __b = &mut n;
    for __i in 0..2 {
        __touch(__b);
        println!("{n}");
    }
    n += 5;
    println!("{} {n}", c());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

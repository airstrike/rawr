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
    println!("{n}");
    let __s = &n;
    n += 5;
    __see(__s);
    println!("{} {n}", c());
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

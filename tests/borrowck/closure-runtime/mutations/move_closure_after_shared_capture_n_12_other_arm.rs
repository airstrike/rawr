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
    if __c() {
        __touch(__b);
    } else {
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

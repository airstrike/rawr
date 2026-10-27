fn main() {
    let mut v = vec![1];
    let r = &mut v;
    let mut n = 0;
    loop {
        r.push(1);
        n += v.len();
        if n > 3 {
            break;
        }
    }
    if __c() {
        let __m = move || __see(&n);
        __m();
    }
    println!("{n}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

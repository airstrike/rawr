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
    let mut __o = Some(&n);
    __see(&__o);
    __o = None;
    println!("{n}");
    __see(&__o);
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);

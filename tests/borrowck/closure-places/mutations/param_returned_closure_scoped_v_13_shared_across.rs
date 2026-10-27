fn pusher(r: &mut Vec<i32>) -> impl FnMut(i32) + '_ {
    move |x| r.push(x)
}

fn main() {
    let mut v = vec![0];
    {
        let mut p = pusher(&mut v);
        p(1);
        p(2);
    }
    v.push(3);
    let __s = &v;
    println!("{v:?}");
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

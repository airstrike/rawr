fn pusher(r: &mut Vec<i32>) -> impl FnMut(i32) + '_ {
    move |x| r.push(x)
}

fn main() {
    let mut v = vec![0];
    {
        let mut p = pusher(&mut v);
        p(1);
        let __s = &p;
        p(2);
        __see(__s);
    }
    v.push(3);
    println!("{v:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

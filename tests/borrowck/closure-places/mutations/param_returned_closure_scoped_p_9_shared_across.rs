fn pusher(r: &mut Vec<i32>) -> impl FnMut(i32) + '_ {
    move |x| r.push(x)
}

fn main() {
    let mut v = vec![0];
    {
        let mut p = pusher(&mut v);
        let __s = &p;
        p(1);
        __see(__s);
        p(2);
    }
    v.push(3);
    println!("{v:?}");
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

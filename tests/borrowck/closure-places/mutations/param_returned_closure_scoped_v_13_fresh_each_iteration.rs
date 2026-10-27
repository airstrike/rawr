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
    for __i in 0..2 {
        let __b = &mut v;
        __touch(__b);
        println!("{v:?}");
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

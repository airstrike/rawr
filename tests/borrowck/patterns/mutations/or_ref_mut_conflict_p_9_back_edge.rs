fn main() {
    let mut p: (Option<i64>, i64) = (Some(1), 2);
    match p {
        (Some(ref mut x), _) | (None, ref mut x) => {
            p.1 = 5;
            *x += 1;
        }
    }
    let __b = &mut p;
    for __i in 0..2 {
        __touch(__b);
        println!("{:?}", p);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

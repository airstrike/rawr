fn main() {
    let mut p: (Option<i64>, i64) = (Some(1), 2);
    match p {
        (Some(ref mut x), _) | (None, ref mut x) => {
            *x += 1;
            p.1 = 5;
        }
    }
    let __b = &mut p;
    if __c() {
        __touch(__b);
    } else {
        println!("{:?}", p);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

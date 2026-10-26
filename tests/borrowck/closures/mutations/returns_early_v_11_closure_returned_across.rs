fn main() {
    let mut v = vec![1, 2];
    let w = vec![3];
    let c = |flag: bool| {
        if flag {
            return &v;
        }
        &w
    };
    let r = c(true);
    let __h = || &v;
    let __k = __h();
    v.push(3);
    __see(__k);
    println!("{:?}", r);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

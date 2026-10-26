fn main() {
    let mut v = vec![1];
    for i in 0..3 {
        let mut c = || v.push(i);
        let __h = || &c;
        let __k = __h();
        c();
        __see(__k);
        println!("{}", v.len());
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

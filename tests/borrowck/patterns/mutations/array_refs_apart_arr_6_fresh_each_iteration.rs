fn main() {
    let mut arr = [1, 2, 3];
    let [ref mut a, .., ref mut b] = arr;
    *a += 10;
    *b += 20;
    for __i in 0..2 {
        let __b = &mut arr;
        __touch(__b);
        println!("{:?}", arr);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

fn main() {
    let mut arr = [1, 2, 3];
    let [ref mut a, ..] = arr;
    arr[1] = 5;
    *a += 10;
    let __b = &mut arr;
    for __i in 0..2 {
        __touch(__b);
        println!("{:?}", arr);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

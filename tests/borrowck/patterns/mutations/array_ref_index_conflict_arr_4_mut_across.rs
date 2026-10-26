fn main() {
    let mut arr = [1, 2, 3];
    let [ref mut a, ..] = arr;
    let __b = &mut arr;
    arr[1] = 5;
    __touch(__b);
    *a += 10;
    println!("{:?}", arr);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

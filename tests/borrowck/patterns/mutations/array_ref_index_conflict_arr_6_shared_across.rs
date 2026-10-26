fn main() {
    let mut arr = [1, 2, 3];
    let [ref mut a, ..] = arr;
    arr[1] = 5;
    *a += 10;
    let __s = &arr;
    println!("{:?}", arr);
    __see(__s);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

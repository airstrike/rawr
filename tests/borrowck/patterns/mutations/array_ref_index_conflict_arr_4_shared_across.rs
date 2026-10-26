fn main() {
    let mut arr = [1, 2, 3];
    let [ref mut a, ..] = arr;
    let __s = &arr;
    arr[1] = 5;
    __see(__s);
    *a += 10;
    println!("{:?}", arr);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

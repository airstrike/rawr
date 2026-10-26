fn main() {
    let v = vec![1, 2, 3];
    let mut k = 1;
    let it = v.iter().map(|x| x + k);
    k += 1;
    let total: i32 = it.sum();
    let __h = || &k;
    let __k = __h();
    println!("{} {}", total, k);
    __see(__k);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

fn main() {
    let v = vec![1, 2, 3];
    let mut k = 1;
    let it = v.iter().map(|x| x + k);
    let __m = move || __see(&k);
    k += 1;
    __m();
    let total: i32 = it.sum();
    println!("{} {}", total, k);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

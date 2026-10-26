fn main() {
    let v = vec![1, 2, 3];
    let mut k = 1;
    let it = v.iter().map(|x| x + k);
    k += 1;
    let total: i32 = it.sum();
    println!("{} {}", total, k);
    let __m = move || __see(&k);
    __m();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

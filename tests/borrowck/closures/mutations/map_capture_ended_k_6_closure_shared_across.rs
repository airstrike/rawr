fn main() {
    let v = vec![1, 2, 3];
    let mut k = 1;
    let total: i32 = v.iter().map(|x| x + k).sum();
    k += 1;
    let __g = || __see(&k);
    println!("{} {}", total, k);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

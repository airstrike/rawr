fn main() {
    let v = vec![1, 2, 3];
    let mut k = 1;
    let it = v.iter().map(|x| x + k);
    k += 1;
    let total: i32 = it.sum();
    let __f = || __see(&k);
    let __g = __f;
    __f();
    println!("{} {}", total, k);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

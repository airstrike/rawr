fn main() {
    let mut v = vec![1, 2, 3];
    let k = 2;
    let doubled: Vec<i64> = v.iter().map(|x| x * k).collect();
    v.push(doubled.len() as i64);
    let __m = move || __see(&v);
    __m();
    println!("{:?} {:?}", v, doubled);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

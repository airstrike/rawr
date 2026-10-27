fn main() {
    let mut v = vec![1, 2];
    let mut o: Option<usize> = None;
    let n = *o.get_or_insert_with(|| v.len());
    if __c() {
        let __m = move || __see(&v);
        __m();
    }
    v.push(3);
    println!("{} {:?} {:?}", n, o, v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

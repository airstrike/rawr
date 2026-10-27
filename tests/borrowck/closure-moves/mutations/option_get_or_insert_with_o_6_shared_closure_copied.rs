fn main() {
    let mut v = vec![1, 2];
    let mut o: Option<usize> = None;
    let n = *o.get_or_insert_with(|| v.len());
    v.push(3);
    let __f = || __see(&o);
    let __g = __f;
    __f();
    println!("{} {:?} {:?}", n, o, v);
    __g();
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

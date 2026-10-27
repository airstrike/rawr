fn main() {
    let mut v = vec![1, 2];
    let mut o: Option<usize> = None;
    let n = *o.get_or_insert_with(|| v.len());
    let mut __f = || __touch(&mut v);
    let mut __g = __f;
    __g();
    __f();
    v.push(3);
    println!("{} {:?} {:?}", n, o, v);
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

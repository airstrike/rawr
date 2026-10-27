fn main() {
    let mut v = vec![1, 2];
    let mut o: Option<usize> = None;
    let n = *o.get_or_insert_with(|| v.len());
    v.push(3);
    let mut __f = || __touch(&mut o);
    for __i in 0..2 {
        __f();
        println!("{} {:?} {:?}", n, o, v);
    }
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

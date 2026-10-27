fn main() {
    let a = 1;
    let b = 2;
    let mut v = vec![&a, &b];
    let x = v.iter().cloned().next().unwrap();
    let __k = Some(0).map(|_| &v);
    v.clear();
    __see(&__k);
    println!("{x}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);

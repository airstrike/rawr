fn main() {
    let a = 3;
    let b = 4;
    let mut v = vec![&a, &b];
    let m = v.iter().min().unwrap();
    let __k = Some(0).map(|_| &v);
    v.clear();
    __see(&__k);
    println!("{m}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);

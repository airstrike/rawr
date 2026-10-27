fn main() {
    let a = 1;
    let b = 2;
    let mut v = vec![&a, &b];
    let x = v.iter().cloned().next().unwrap();
    let __v = vec![&v];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    v.clear();
    println!("{x}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);

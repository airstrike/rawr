fn main() {
    let a = 3;
    let b = 4;
    let mut v = vec![&a, &b];
    let m = v.iter().min().unwrap();
    let __v = vec![&v];
    let __k = __v.iter().copied().next().unwrap();
    v.clear();
    __see(&__k);
    println!("{m}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);

fn main() {
    let a = 3;
    let b = 4;
    let mut v = vec![&a, &b];
    let m = v.iter().copied().max().unwrap();
    let __v = vec![&v];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    v.clear();
    println!("{m}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);

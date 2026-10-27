fn main() {
    let a = 3;
    let mut v = vec![&a];
    let (i, x) = v.iter().copied().enumerate().next().unwrap();
    let __v = vec![&v];
    let __k = __v.iter().copied().next().unwrap();
    v.clear();
    __see(&__k);
    println!("{i} {x}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);

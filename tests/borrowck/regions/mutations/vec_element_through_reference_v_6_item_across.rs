fn main() {
    let a = 1;
    let mut v = vec![&a];
    let r = &v;
    let x = r[0];
    let __v = vec![&v];
    let __k = __v.iter().copied().next().unwrap();
    v.push(&a);
    __see(&__k);
    println!("{x}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);

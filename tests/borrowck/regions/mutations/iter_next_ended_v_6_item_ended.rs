fn main() {
    let a = 1;
    let mut v = vec![&a];
    let x = v.iter().next().unwrap();
    println!("{x}");
    let __v = vec![&v];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    v.push(&a);
    println!("{}", v.len());
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);

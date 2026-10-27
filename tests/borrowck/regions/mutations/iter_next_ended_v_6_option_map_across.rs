fn main() {
    let a = 1;
    let mut v = vec![&a];
    let x = v.iter().next().unwrap();
    println!("{x}");
    let __k = Some(0).map(|_| &v);
    v.push(&a);
    __see(&__k);
    println!("{}", v.len());
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);

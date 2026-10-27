fn main() {
    let a = 1;
    let mut v = vec![&a];
    let w = v.clone();
    let __k = Some(0).map(|_| &v);
    v.push(&a);
    __see(&__k);
    println!("{w:?}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);

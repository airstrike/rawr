fn main() {
    let mut a = 1;
    let v = vec![&a];
    let w = v.clone();
    let __k = Some(0).map(|_| &a);
    a += 1;
    __see(&__k);
    println!("{w:?}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);

fn main() {
    let mut a = 1;
    let mut v = vec![&a];
    let x = v.pop().unwrap();
    let __k = Some(0).map(|_| &a);
    a += 1;
    __see(&__k);
    println!("{x}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);

fn main() {
    let mut a = 3;
    let b = 4;
    let v = vec![&a, &b];
    let m = v.iter().copied().max().unwrap();
    let __k = Some(0).map(|_| &a);
    a += 1;
    __see(&__k);
    println!("{m} {a}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);

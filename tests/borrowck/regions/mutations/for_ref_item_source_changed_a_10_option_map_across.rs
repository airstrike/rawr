fn main() {
    let mut a = 3;
    let b = 4;
    let v = vec![&a, &b];
    let mut best = &0;
    for x in &v {
        best = *x;
    }
    a += 1;
    let __k = Some(0).map(|_| &a);
    println!("{best} {a}");
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);

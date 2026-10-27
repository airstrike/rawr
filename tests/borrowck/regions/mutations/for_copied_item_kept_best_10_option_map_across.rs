fn main() {
    let a = 3;
    let b = 4;
    let mut v = vec![&a, &b];
    let mut best = &0;
    for x in v.iter().copied() {
        best = x;
    }
    v.clear();
    let __k = Some(0).map(|_| &best);
    println!("{best}");
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);

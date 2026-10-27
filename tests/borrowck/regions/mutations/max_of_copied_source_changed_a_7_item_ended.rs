fn main() {
    let mut a = 3;
    let b = 4;
    let v = vec![&a, &b];
    let m = v.iter().copied().max().unwrap();
    a += 1;
    let __v = vec![&a];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    println!("{m} {a}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);

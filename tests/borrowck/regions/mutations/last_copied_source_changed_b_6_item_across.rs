fn main() {
    let a = 3;
    let mut b = 4;
    let v = vec![&a, &b];
    let x = v.iter().copied().last().unwrap();
    let __v = vec![&b];
    let __k = __v.iter().copied().next().unwrap();
    b += 1;
    __see(&__k);
    println!("{x} {b}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);

fn main() {
    let mut a = 1;
    let mut v = vec![&a];
    let x = v.pop().unwrap();
    let __v = vec![&a];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    a += 1;
    println!("{x}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);

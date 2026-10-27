fn main() {
    let mut x = 1;
    let r = &x;
    let y = *r;
    x = 2;
    let __v = vec![&x];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    println!("{}", x + y);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);

fn main() {
    let mut x = 5;
    let r = &mut x;
    let y = x;
    *r += 1;
    let __v = vec![&x];
    let __k = __v.iter().copied().next().unwrap();
    println!("{} {}", x, y);
    __see(&__k);
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);

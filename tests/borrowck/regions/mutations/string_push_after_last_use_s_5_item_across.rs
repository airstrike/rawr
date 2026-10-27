fn main() {
    let mut s = String::from("a");
    let r = &s;
    let n = r.len();
    let __v = vec![&s];
    let __k = __v.iter().copied().next().unwrap();
    s.push('b');
    __see(&__k);
    println!("{n} {s}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);

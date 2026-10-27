fn main() {
    let mut a = String::from("a");
    let o = Some(&a);
    let n = o.map(|s| s.len());
    let __v = vec![&a];
    let __k = __v.iter().copied().next().unwrap();
    a.push_str("!");
    __see(&__k);
    println!("{n:?} {a}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);

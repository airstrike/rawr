fn main() {
    let mut a = String::from("a");
    let o = Some(&a);
    let n = o.map(|s| s.len());
    a.push_str("!");
    let __v = vec![&a];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    println!("{n:?} {a}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);

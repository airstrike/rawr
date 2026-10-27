fn main() {
    let mut s = String::from("a");
    let r = &s;
    let __v = vec![&s];
    let __k = __v.iter().copied().next().unwrap();
    __see(&__k);
    s.push('b');
    println!("{r}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);

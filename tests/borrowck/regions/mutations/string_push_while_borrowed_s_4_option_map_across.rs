fn main() {
    let mut s = String::from("a");
    let r = &s;
    let __k = Some(0).map(|_| &s);
    s.push('b');
    __see(&__k);
    println!("{r}");
}

fn __see<T>(_: &T) {}
struct __Two<'a, 'b, A, B>(&'a A, &'b B);

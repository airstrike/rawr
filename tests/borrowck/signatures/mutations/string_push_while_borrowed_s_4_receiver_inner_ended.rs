fn main() {
    let mut s = String::from("a");
    let r = &s;
    let __t = vec![&s].to_vec();
    __see(&__t);
    s.push('b');
    println!("{r}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

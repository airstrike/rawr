fn main() {
    let mut a = 1;
    let refs = vec![&a];
    let first = refs[0];
    let __t = vec![&a].to_vec();
    a += 1;
    __see(&__t);
    println!("{first}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

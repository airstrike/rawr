struct View<'a> {
    value: &'a i32,
}

fn main() {
    let mut a = 1;
    let view = View { value: &a };
    let moved = view;
    let __t = vec![&a].to_vec();
    a += 1;
    __see(&__t);
    println!("{}", moved.value);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

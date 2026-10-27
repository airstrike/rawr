fn make(step: i32) -> impl Fn(i32) -> i32 {
    move |x| x + step
}

fn main() {
    let mut f = make(1);
    let g = &f;
    println!("{} {}", f(1), g(2));
    let __b = &mut f;
    f = make(10);
    __touch(__b);
    println!("{}", f(1));
}

fn __touch<T>(_: &mut T) {}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

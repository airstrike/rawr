fn main() {
    let mut x = 1;
    let r = &x;
    let y = *r;
    for __i in 0..2 {
        if __c() {
            __take(x);
        }
    }
    x = 2;
    println!("{}", x + y);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}

fn main() {
    let mut s = String::from("a");
    let r = &s;
    for __i in 0..2 {
        let __m = s;
        s = __m;
    }
    s.push('b');
    println!("{r}");
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}

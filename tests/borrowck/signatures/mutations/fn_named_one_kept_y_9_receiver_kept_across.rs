fn pick<'a, 'b>(a: &'a str, _b: &'b str) -> &'a str {
    a
}

fn main() {
    let x = String::from("kept");
    let mut y = String::from("free");
    let p = pick(&x, &y);
    let __t = vec![&y].first();
    y.push_str("!");
    __see(&__t);
    println!("{p} {y}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

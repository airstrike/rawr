fn name(_v: &Vec<i32>) -> &'static str {
    "vec"
}

fn main() {
    let mut v = vec![1];
    let n = name(&v);
    v.push(2);
    __see(&vec![&v].first());
    println!("{n} {v:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

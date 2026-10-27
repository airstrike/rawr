fn name(_v: &Vec<i32>) -> &'static str {
    "vec"
}

fn main() {
    let mut v = vec![1];
    let n = name(&v);
    v.push(2);
    let __t = vec![&v].to_vec();
    println!("{n} {v:?}");
    __see(&__t);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

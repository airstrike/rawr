fn name(_v: &Vec<i32>) -> &'static str {
    "vec"
}

fn main() {
    let mut v = vec![1];
    let n = name(&v);
    let __v = vec![&v];
    v.push(2);
    __see(&__v);
    println!("{n} {v:?}");
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);

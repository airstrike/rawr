fn sum(v: &Vec<i32>) -> i32 {
    v.iter().sum()
}

fn main() {
    let mut v = vec![1, 2];
    let s = sum(&v);
    v.push(s);
    let __k = __keep(&v);
    println!("{v:?}");
    if __c() {
        __see(__k);
    }
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);

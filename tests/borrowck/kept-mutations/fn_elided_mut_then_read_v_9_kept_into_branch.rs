fn last(v: &mut Vec<i32>) -> &mut i32 {
    let n = v.len();
    &mut v[n - 1]
}

fn main() {
    let mut v = vec![1, 2, 3];
    let l = last(&mut v);
    let __k = __keep(&v);
    println!("{}", v.len());
    if __c() {
        __see(__k);
    }
    *l += 1;
}

fn __keep<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}
struct __Hold<'a, T>(&'a T);

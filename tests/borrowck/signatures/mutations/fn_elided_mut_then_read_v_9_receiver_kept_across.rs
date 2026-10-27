fn last(v: &mut Vec<i32>) -> &mut i32 {
    let n = v.len();
    &mut v[n - 1]
}

fn main() {
    let mut v = vec![1, 2, 3];
    let l = last(&mut v);
    let __t = vec![&v].first();
    println!("{}", v.len());
    __see(&__t);
    *l += 1;
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

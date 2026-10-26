fn sum(v: &Vec<i32>) -> i32 {
    v.iter().sum()
}

fn main() {
    let mut v = vec![1, 2];
    let s = sum(&v);
    v.push(s);
    let __t = &__id(&v);
    __see(__t);
    println!("{v:?}");
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

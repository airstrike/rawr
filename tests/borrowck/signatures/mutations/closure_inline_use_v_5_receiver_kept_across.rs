fn main() {
    let mut v = vec![1, 2, 3];
    let k = 2;
    let doubled: Vec<i64> = v.iter().map(|x| x * k).collect();
    let __t = vec![&v].first();
    v.push(doubled.len() as i64);
    __see(&__t);
    println!("{:?} {:?}", v, doubled);
}

fn __id<T>(x: &T) -> &T {
    x
}
fn __see<T>(_: &T) {}
fn __c() -> bool {
    false
}

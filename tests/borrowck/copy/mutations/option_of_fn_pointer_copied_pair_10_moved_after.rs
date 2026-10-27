fn double(x: i32) -> i32 {
    x * 2
}

fn main() {
    let maybe: Option<fn(i32) -> i32> = Some(double);
    let pair = (maybe, [maybe; 2]);
    let copy = pair;
    let used = pair.0.map(|f| f(3));
    println!("{:?} {:?} {}", used, copy.1[1].map(|f| f(4)), pair.1.len());
    __take(pair);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}

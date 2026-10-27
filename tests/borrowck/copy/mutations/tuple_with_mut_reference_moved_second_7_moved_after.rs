fn main() {
    let mut list = vec![1, 2, 3];
    let pair = (&mut list, 1);
    let first = pair;
    let second = pair;
    first.0.push(4);
    println!("{}", second.1);
    __take(second);
}

fn __take<T>(_: T) {}
fn __c() -> bool {
    false
}
